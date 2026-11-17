# Type-in answers

Spec: render/type-answer
Requirements: R532, R533, R537, R964 (comparison side), R977 (comparison side), R941 (comparison inputs; preview wiring in U71); verifies AE17 (comparison part)
Anki: 26.09.3
Scenarios: conformance/scenarios/render/type-answer/

What review does with the type-in markers that the template engine leaves in a card's sides (`render/filters#F7`): the input box on the question, the warnings, the comparison of the typed text with the expected text on the answer, and the comparison HTML itself. Every output here equals the oracle's string for string (R537), except where A9 (Meguri's own modifiers) and A4.4 (the display-only input attributes of R533) say otherwise. The observations that expose these functions to scenarios are `type_answer` and `review_html` (`specs/conformance/type-answer-ops.md`).

Pattern conventions ("shortest", leftmost-first) are `text/strip-html#H1`.

## A1. Terms

- **Side text**: a card's question or answer as the render pipeline gives it for review (`render/card-render-pipeline`, U62): the note type's CSS in a `<style>` element, then the side's HTML, with play markers (`[anki:play:q:0]`, `[anki:play:a:0]`, ...) where AV tags were, and the type-in markers as the template engine wrote them. The answer's `{{FrontSide}}` holds the question's HTML, markers included.
- **Field**: a field of the card's note type. Its **font** and **size** are the field config's `font_name` and `font_size` (schema-11 `font` and `size`, `model/schema11-json#S5`). A new field gets `Arial` and 20.
- **Card number**: the card's template ordinal plus one (`render/cloze#C1`).
- **Typed text**: what the `#typeans` input holds when the answer is revealed (R536, U67). Any string; an input element never yields a line feed. When the question shows no box it is the empty string.
- **Type-in** of a card: what the question side decides for the answer (A4): the expected text, the comparison mode (A6), and the font and size. A card may have none.

## A2. Markers

A **marker** is `[[type:`, then its **spec**: one or more characters, none of them U+000A, as few as possible, then `]]`. Markers are found left to right; after a marker, the search resumes after its `]]`.

- Markers are searched in the whole side text, not only where the template engine wrote them. A field whose text is `[[type:Front]]`, shown with `{{Field}}`, gives a marker; so does that text written in a template outside any `{{…}}`.
- The template engine writes `[[type:` KEY `]]`, `[[type:nc:` KEY `]]` and `[[type:cloze:` KEY `]]` (`render/filters#F7`).

| Text | Markers (spec) |
|---|---|
| `[[type:Back]]` | `Back` |
| `x[[type:nc:Back]]y[[type:Back]]` | `nc:Back`, then `Back` |
| `[[type:a]]]` | `a` (the last `]` is text) |
| `[[type:]]x]]` | `]]x` (the spec needs one character, so the first `]]` cannot end it) |
| `[[type:a` LF `b]]`, `[[type:]]`, `[type:a]` | none |

## A3. Where it runs

Review builds each side it shows in this order:

1. the side text (A1);
2. the media name encoding of `render/media-display#D1`;
3. the play buttons of the display transform (`render/display-transform`, U62);
4. on the question the question filter (A4), on the answer the answer filter (A5);
5. the input attributes of A4.4 (display only).

The `review_html` observation runs steps 1, 2 and 4: play markers stay as markers and the box has no R533 attributes. Leaving step 3 out cannot change step 4 unless a marker's spec holds a play marker, which no real template writes.

Steps 2 to 4 see the whole side text, its `<style>` element included, as in Anki. `render/display-transform#D2` (U62) runs them on the side without that element and adds it last. The output is the same unless the note type's CSS itself holds a marker (A2), the exact text `<hr id=answer>` or a media tag with a local name to encode (`render/media-display#D2`); for such CSS the order here holds, so the steps must also see the CSS. `render/type-answer/whole-side` fixes this order, including a marker, an exact answer rule and a local media tag in the CSS.

## A4. Question side (R532)

### A4.1 Reading the first marker

Only the first marker of the question decides. With no marker, the question is unchanged and the card has no type-in. Otherwise, with S the first marker's spec:

1. **Cloze.** When S starts with `cloze:`, the card is a cloze type-in, and S becomes the text between the first and the second `:` of S (up to the end when there is no second `:`).
2. **nc.** Then, when S starts with `nc:`, the comparison ignores combining characters (mode `nc`, A6), and S becomes the text between its first and second `:` in the same way.
3. S is the **field name**.

The two rules apply once each, in that order. So a cloze type-in never has mode `nc` in Anki: `cloze:nc:Text` names a field `nc`.

| Spec | Field name | Cloze | Mode |
|---|---|---|---|
| `Back` | `Back` | no | plain |
| `nc:Back` | `Back` | no | nc |
| `cloze:Text` | `Text` | yes | plain |
| `cloze:Text:x` | `Text` | yes | plain |
| `nc:a:b` | `a` | no | nc |
| `cloze:nc:Text` | `nc` | yes | plain |
| `nc:cloze:Text` | `cloze` | no | nc |
| `nc:` | (empty) | no | nc |

Meguri also reads modifiers from the question template (A9).

### A4.2 The expected text

- The field is the first field of the card's note type whose name equals the field name exactly (case included). There may be none: special names such as `Tags` or `Deck` are not fields, and a marker written in a template or a field can name anything.
- When the field exists, the **expected text** is the note's value of that field as stored. For a cloze type-in it is the cloze type-in extraction of that value for the card number (`render/cloze#C10`), and an empty extraction counts as if the field did not exist. The font and size are always the field's.

### A4.3 Outcomes

1. **No field** (or, for a cloze type-in, an empty extraction): every marker of the question is replaced by a warning (A8): the cloze warning for a cloze type-in, otherwise the unknown-field warning with the field name. The card has no type-in.
2. **Empty expected text** (not a cloze type-in): the field's value is the empty string. Every marker of the question is removed. The card has no type-in. A value that only looks empty is not empty here: `<br>`, a space or `&nbsp;` gives the box, and the comparison then expects the empty string (A6.1).
3. **Otherwise** every marker of the question is replaced by the box (A4.4). All markers, not just the first (subject to A4.5 replacement expansion): `{{type:Back}}|{{type:Extra}}` gives two identical boxes with Back's font and size. Only the first one's input is read at reveal (U67), so later boxes do nothing. The card's type-in is the expected text, the mode and the field's font and size.

### A4.4 The box

The box is a U+000A followed by these four lines, each ended by U+000A (so it starts with an empty line and ends with a line break), with exactly three spaces before `style`:

```

<center>
<input type=text id=typeans onkeypress="_typeAnsPress();"
   style="font-family: 'FONT'; font-size: SIZEpx;">
</center>
```

FONT is the field's font as stored (not escaped: a `'` in it ends the CSS string), SIZE its size in decimal. `_typeAnsPress` is the card page's function (U66).

Example: template `{{Front}}<br>{{type:Back}}`, Front `cat`, Back's font `Qav Serif` and size 28, CSS `.card { text-align: center; }`:

```
<style>.card { text-align: center; }</style>cat<br>
<center>
<input type=text id=typeans onkeypress="_typeAnsPress();"
   style="font-family: 'Qav Serif'; font-size: 28px;">
</center>

```

**Display form (R533).** The box Meguri shows has ` autocapitalize="off" autocorrect="off" spellcheck="false"` inserted right after `id=typeans`, so its second line reads `<input type=text id=typeans autocapitalize="off" autocorrect="off" spellcheck="false" onkeypress="_typeAnsPress();"`. Anki's box has no such attributes. This is step 5 of A3; the `review_html` observation compares the box without them, so the difference has no golden and no deviation entry, and Meguri's own tests assert it.

### A4.5 Question replacement escapes

For pinned parity (R1907), the completed question box or warning is a replacement template. Its stored font or missing-field name can therefore contain escape syntax. Parse that template for each marker match; do not interpret the answer wrapper or comparison as a replacement template.

- `\` inserts one backslash. `\a`, `\b`, `\f`, `\n`, `\r`, `\t` and `\v` insert U+0007, U+0008, U+000C, U+000A, U+000D, U+0009 and U+000B.
- `\g<0>` inserts the complete marker; `\g<1>` or `\1` inserts that match's marker spec. The pattern has only this one capture. Expansion is per match, without recursively interpreting inserted capture text. Thus a capture escape in a font can make later boxes differ although their descriptor is the first marker's.
- A backslash followed by zero begins an octal escape, consuming at most three octal digits in total. Three octal digits beginning with 1 through 7 also form an octal escape. Its value must fit 0–255. Otherwise a one- or two-digit nonzero decimal escape denotes a capture, which must exist.
- A backslash before punctuation or any other unrecognized non-ASCII-letter, nondigit character is retained together with that character. Other unknown ASCII-letter escapes, invalid capture names or indices, unterminated `\g<...>`, an octal value above 255, and a terminal backslash refuse the replacement.

A refusal is the typed error `invalid_type_answer_replacement`. It never panics, changes collection data or returns partially transformed HTML. `review_html` returns that error for the affected case; the oracle maps only its replacement parser's errors to it, without message text. The caller can show a safe rendering error. Valid cases and malformed imported fonts are fixed by `render/type-answer/replacement-escapes`. The answer's font remains literal, including any backslashes (A5).

## A5. Answer side (R537)

Input: the answer text after A3 steps 1 to 3, the card's type-in from its question (A4), and the typed text.

1. **No type-in**: every marker of the answer is removed. Nothing else changes.
2. Otherwise let C be the comparison (A6) of the expected text and the typed text in the type-in's mode, let B be U+000A + `<div style="font-family: '` FONT `'; font-size: ` SIZE `px">` + C + `</div>` (no `;` after `px`), and let R be the answer with every occurrence of the exact text `<hr id=answer>` removed.
   1. When the answer has no `<hr id=answer>`: every marker of the answer is replaced by B.
   2. When it has one or more and R has no marker: the answer stays exactly as it was, rules included.
   3. Otherwise every marker of R is replaced by `<hr id=answer>` + B.

Consequences:

- The rule moves before the comparison wherever it was: after the box (`{{FrontSide}}<hr id=answer>{{Back}}`, the usual case), before it, or both. Two rules become one per marker.
- Only that exact text counts: `<hr id="answer">`, `<HR id=answer>` and `<hr id=answer />` stay where they are and get no copy.
- The comparison always uses the question's field: an answer marker naming another field (`{{type:Country}}` on an answer whose question had `{{type:Capital}}`) shows the comparison for Capital.
- A marker that only the removal forms counts: `[[type:<hr id=answer>X]]` in an answer becomes the marker `[[type:X]]` of R.
- Every marker gets the same B.

Examples, with the question `1. {{Country}} {{type:Capital}}` (no CSS), Country `Spain`, Capital `Madrid`, typed `Madrid` (G is `<code id=typeans><span class=typeGood>Madrid</span></code>`, D is `<div style="font-family: 'Arial'; font-size: 20px">`):

| Answer template | Answer |
|---|---|
| `{{FrontSide}}<hr id=answer>{{Capital}}` | `<style></style>1. Spain <hr id=answer>` LF D G `</div>Madrid` |
| `{{Country}}<hr id=answer>{{FrontSide}}` | `<style></style>Spain1. Spain <hr id=answer>` LF D G `</div>` |
| `{{FrontSide}}<br>{{Capital}}` | `<style></style>1. Spain ` LF D G `</div><br>Madrid` |
| `{{FrontSide}}<hr id="answer">{{Capital}}` | `<style></style>1. Spain ` LF D G `</div><hr id="answer">Madrid` |
| `{{Country}}<hr id=answer>{{Capital}}` | `<style></style>Spain<hr id=answer>Madrid` (2.2: no marker) |
| `{{Country}}<hr id=answer>{{type:Country}}` | `<style></style>Spain<hr id=answer>` LF D G `</div>` |

## A6. The comparison

The comparison of an expected text and a typed text in mode `plain` (combining characters count) or `nc` (they do not). It is a function of these three inputs alone.

### A6.1 The expected text as compared

The expected text goes through these steps, in order; the result is E.

1. **AV tags** are removed as A6.1.1 says.
2. **Line breaks.** Every longest run made of U+000A characters, `<br>` forms and `<div>` or `</div>` becomes one space. A `<br>` form is `<br`, at most one white-space character (Unicode White_Space), at most one `/`, then `>`. Letters match in any case. `<div class=x>` is no line break (step 3 removes it without a space).
3. **Markup and entities**: plain stripping, `text/strip-html#H4` (markup removed, then entities decoded all or nothing, then U+00A0 to a space).
4. **Trim**: Unicode White_Space is removed at both ends (U+3000 and U+00A0 included).

| Expected text | E |
|---|---|
| `<div>456</div>` | `456` |
| `[sound:qavtone.ogg]<i>7</i> &nbsp;8` | `7  8` (two spaces) |
| `  a<br>b` LF `<div>c</div> ` | `a b c` |
| `a<BR/>b<br />c<br` TAB `>d<DIV>e</Div>f<div class=x>g` | `a b c d e fg` |
| `one<br><br>` LF `<div>two</div>` | `one two` |
| `a & b <b>c</b>` | `a & b c` (one invalid entity: nothing decoded) |
| `cd <path>/x && ls` | `cd /x && ls` (`<path>` is markup) |
| `[anki:tts lang=en_US]hello[/anki:tts]world` | `world` |
| `<br>`, ` `, `&nbsp;`, empty | empty |

### A6.1.1 AV removal

This is the reading of AV tags that the render pipeline uses (`render/av-tags`, U62); the two must agree. The text is read from the start. At its start and at each `[`, the first of these that matches is taken; any other character is text:

- a **sound tag**: `[sound:`, one or more characters other than `]`, then `]`;
- a **directive**: `[anki:`, its NAME (one or more characters other than `]`, space, TAB, CR and LF), white space (space, TAB, CR, LF; possibly none), then options, each a KEY (one or more characters other than `]`, white space and `=`), `=`, a VALUE (either `"`, any characters other than `"`, `"`; or any characters, possibly none, other than `]`, white space and `"`) and possibly white space; then `]`; then the content up to the first `[/anki:` NAME `]`, which ends it. Without that closing text it is no directive.

When the text has neither, it stays exactly as it is. Otherwise it is written again: text as it is, sound tags and directives named exactly `tts` not at all, and any other directive as `[anki:` NAME, then for each distinct KEY a space, KEY, `=` and its last VALUE (in `"` when the value contains `]`, a space, TAB, CR or LF), then `]`, the content as it was (sound tags in it stay), and `[/anki:` NAME `]`. With two or more distinct keys, the order Anki writes them in varies from run to run; Meguri writes them in the order of their first appearance, and no scenario has such a directive.

| Text | After AV removal |
|---|---|
| `a[sound:b]c[anki:tts lang=x]d[/anki:tts]e` | `ace` |
| `[sound:]x`, `[anki:foo]x[/anki:bar]` | unchanged (no tag) |
| `[sound:a]b]c` | `b]c` |
| `[anki:tts]x[/anki:tts]y` | `y` |
| `[anki:foo  a='b']x[/anki:foo]` | `[anki:foo a='b']x[/anki:foo]` |
| `[anki:foo k=""]z[/anki:foo]` | `[anki:foo k=]z[/anki:foo]` |
| `[anki:foo a=1 a=2]x[/anki:foo]` | `[anki:foo a=2]x[/anki:foo]` |
| `[anki:foo][sound:x.mp3][/anki:foo]y` | unchanged |

### A6.2 Empty typed text

`<code id=typeans>` + M(E) + `</code>`, with M the minimal escaping of A6.6. E is shown as it is, without normalization: `<code id=typeans>a &amp; b c</code>`; with an empty E, `<code id=typeans></code>`.

### A6.3 The two sequences

The comparison works on two sequences of characters, T (typed) and X (expected), and keeps a display text for the case where they are equal.

- **plain**: T is the NFC form of the typed text; X is the NFC form of E. Each element of X displays as itself. The display text is NFC(E).
- **nc**: T is the NFKD form of the typed text with every mark removed. X comes from the NFKD form of E, read character by character: a character that is not a mark starts a new **entry**; a mark is appended to the last entry, and is dropped when there is no entry yet. X is the sequence of the entries' first characters, and each element of X displays as its whole entry (base and marks). The display text is E itself, not normalized.

A **mark** in this section is a character of General_Category Mn, Mc or Me in Unicode 16.0.0. NFC and NFKD are those of Unicode 16.0.0 too. The pinned `unicode-normalization` 0.1.24 (`nfc`, `nfkd`, `char::is_combining_mark`) implements exactly these.



### A6.4 Equal sequences

When T and X are equal character by character: `<code id=typeans><span class=typeGood>` + M(display text) + `</span></code>`.

### A6.5 Different sequences

1. **Opcodes.** The edit opcodes of T against X are those of `difflib` 0.4.0 (`=`-pinned, KTD2): `SequenceMatcher::new(T, X)` (T first, X second), no junk function, then `get_opcodes()`. Each opcode is `equal`, `delete`, `insert` or `replace` with the ranges i1..i2 of T and j1..j2 of X. Python's `difflib` gives different opcodes for some inputs (A6.7): never port it.
2. **Tokens**, in opcode order:

   | Opcode | Typed token | Expected token |
   |---|---|---|
   | `equal` | `typeGood`: T[i1..i2] | `typeGood`: X[j1..j2] |
   | `delete` | `typeBad`: T[i1..i2] | none |
   | `insert` | `typeMissed`: `-` repeated j2 − j1 times | `typeMissed`: X[j1..j2] |
   | `replace` | `typeBad`: T[i1..i2] | `typeMissed`: X[j1..j2] |

3. **Token text.**
   - Typed tokens, and expected tokens in plain mode: the token's characters. When the first of them is a mark by **Unicode 10.0.0** (General_Category Mn, Mc or Me in the official database; canonical scalar ranges and provenance: `render/unicode10-marks.md`), a U+00A0 is put in front, so the mark does not join the token before it. Characters that became marks after Unicode 10.0.0 (U+08D3, U+1AC1, ...) get none.
   - Expected tokens in nc mode: the display texts of X's elements j1..j2 (their entries), joined; no U+00A0 is added.
   - Then M (A6.6).
4. **Output**: `<code id=typeans>` + the typed tokens + `<br><span id=typearrow>&darr;</span><br>` + the expected tokens + `</code>`, each token written `<span class=` CLASS `>` text `</span>` (class unquoted). Tokens are never merged: two neighbouring `typeGood` tokens stay two spans.

### A6.6 Minimal escaping

M replaces `&` by `&amp;`, `<` by `&lt;`, `>` by `&gt;`, `"` by `&quot;` and `'` by `&#x27;`, and changes nothing else. This is `encode_minimal` of the pinned `htmlescape` 0.3.1. Typed text is escaped, never interpreted: typing `<b>Madrid</b>` against `Madrid` shows `&lt;b&gt;` and `&lt;/b&gt;` as bad tokens.

### A6.7 The pinned matcher

For reviewers; the crate is authoritative. The matcher finds the longest block of equal elements of T and X (the earliest in T among the longest, then the earliest in X for it), keeps it, and does the same on the parts left and right of it, recursively; opcodes then describe the gaps between the kept blocks. It differs from Python's `difflib` when X has 200 elements or more: only the elements that occur more than ⌊n / 100⌋ + 1 times in X (n = X's length) are used to find blocks, and a block found through them is then extended over equal neighbours. With `x`×100 + `abc` + `y`×100 as E and `abc` typed, the matcher finds no block: the whole typed text is one bad token and the whole expected text one missed token, where Python would match `abc`. The corpus `render/type-answer/corpus/` holds 150 such long pairs.

### A6.8 Two Unicode versions

The comparison uses Unicode 16.0.0 for NFC, NFKD and the marks of nc mode (A6.3), and the official Unicode 10.0.0 category ranges of `unicode10-marks.json` for the marks that get a U+00A0 in token texts (A6.5). Meguri must not unify them: `xy` against typed `x` U+08D3 `y` shows a bad token U+08D3 without a U+00A0, while U+0335 in its place gets one.

## A7. Comparison examples

Recorded from the oracle (`render/type-answer/compare`). Rows that do not give the whole HTML list the typed tokens, then `|`, then the expected tokens, each as its class (Good, Bad, Missed for `typeGood`, `typeBad`, `typeMissed`) and its text; a text in quotes shows its spaces. "both" means plain and nc give the same output.

| Expected | Typed | Mode | Output |
|---|---|---|---|
| `ka` | `ka` | both | `<code id=typeans><span class=typeGood>ka</span></code>` |
| `ka` | `kq` | both | `<code id=typeans><span class=typeGood>k</span><span class=typeBad>q</span><br><span id=typearrow>&darr;</span><br><span class=typeGood>k</span><span class=typeMissed>a</span></code>` |
| `ka` | (empty) | both | `<code id=typeans>ka</code>` |
| `example` | `sample` | both | `<code id=typeans><span class=typeBad>s</span><span class=typeGood>ample</span><br><span id=typearrow>&darr;</span><br><span class=typeMissed>ex</span><span class=typeGood>ample</span></code>` |
| `456` | `4456` | both | Bad `4`, Good `456` \| Good `456` |
| `5` | `67` | both | Bad `67` \| Missed `5` |
| `56` | `5` | both | Good `5`, Missed `-` \| Good `5`, Missed `6` |
| `Madrid` | `Madird` | both | Good `Mad`, Missed `-`, Good `i`, Bad `r`, Good `d` \| Good `Mad`, Missed `r`, Good `i`, Good `d` |
| `Hola` | `hola` | both | Bad `h`, Good `ola` \| Missed `H`, Good `ola` |
| `qávul` | `qavul` | plain | `<code id=typeans><span class=typeGood>q</span><span class=typeBad>a</span><span class=typeGood>vul</span><br><span id=typearrow>&darr;</span><br><span class=typeGood>q</span><span class=typeMissed>á</span><span class=typeGood>vul</span></code>` |
| `qávul` | `qavul` | nc | `<code id=typeans><span class=typeGood>qávul</span></code>` |
| `Qëvax` | `qevax` | nc | `<code id=typeans><span class=typeBad>q</span><span class=typeGood>evax</span><br><span id=typearrow>&darr;</span><br><span class=typeMissed>Q</span><span class=typeGood>ëvax</span></code>` |
| `Madrid` | ` Madrid ` | both | Bad `" "`, Good `Madrid`, Bad `" "` \| Good `Madrid` |
| `a<b` | `a<b` | both | `<code id=typeans><span class=typeGood>a&lt;b</span></code>` (`<b` has no `>`, so it is no markup) |
| `<br>` | `x` | both | `<code id=typeans><span class=typeBad>x</span><br><span id=typearrow>&darr;</span><br></code>` |
| `é` | `e` U+0301 | plain | Good `é` (both NFC) |
| `ﬁ` | `fi` | plain | Bad `fi` \| Missed `ﬁ` |
| `ﬁ①` | `fi1` | nc | Good `ﬁ①` |
| U+0301 `ab` | `ab` | plain | Missed `-`, Good `ab` \| Missed U+00A0 U+0301, Good `ab` |
| U+0301 `ab` | `ab` | nc | Good U+0301 `ab` |
| `xy` | `x` U+0335 `y` | plain | Good `x`, Bad U+00A0 U+0335, Good `y` \| Good `x`, Good `y` |
| `xy` | `x` U+08D3 `y` | plain | Good `x`, Bad U+08D3, Good `y` \| Good `x`, Good `y` |
| `❤` | `❤` U+FE0F | plain | Good `❤`, Bad U+00A0 U+FE0F \| Good `❤` |
| `한국` | `한구` | nc | Good `한구` (as five jamo), Missed `-` \| Good `한구` (five jamo), Missed U+11A8 |
| `שָׁלוֹם` | `שלם` | nc | Good `של`, Missed `-`, Good `ם` \| Good `שָׁל`, Missed `וֹ`, Good `ם` |
| `Straße` | `Strasse` | both | Good `Stra`, Bad `ss`, Good `e` \| Good `Stra`, Missed `ß`, Good `e` |
| `x`×100 `abc` `y`×100 | `abc` | both | Bad `abc` \| Missed (the whole expected text) |

## A8. Warnings

| When | Text shown | Token in `review_html` |
|---|---|---|
| A4.3 rule 1, not a cloze type-in | `Type answer: unknown field ` + the field name | `[[warning:unknown_field:` + the field name + `]]` |
| A4.3 rule 1, cloze type-in | `Please run Tools>Empty Cards` | `[[warning:empty_cards]]` |

- The text replaces each marker as it is, without escaping (the field name and the `>` are inserted raw). These are Anki's English strings (R532); other languages come from Meguri's String Catalog.
- Observations never hold localized text (R1887), so both executors write the tokens instead of the text in `review_html`.
- Examples: `{{Front}}{{type:Tags}}` (a special field, not a field) gives `cat[[warning:unknown_field:Tags]]` in the observation and `catType answer: unknown field Tags` on screen; `{{type:cloze:Extra}}` on a card whose Extra holds no cloze for its number gives `[[warning:empty_cards]]`.

## A9. Meguri's modifiers: `ci` and combined modifiers (R964)

Anki 26.09.3 reads only the markers of `render/filters#F7`. Chains such as `type:ci:F`, `type:nc:ci:F` or `type:cloze:nc:F` give the plain marker `[[type:F]]` there, and its reviewer compares case-sensitively, with combining characters, against the whole field. Meguri's template engine writes the same markers (F7), so card HTML still matches Anki. On top of that, Meguri reads modifiers from the question template:

1. Take the field name and the flags (cloze, nc) that A4.1 reads from the first marker.
2. In the card's question template (the template at the card's ordinal; a cloze note type's only template), as the template engine parses it (`render/template-syntax#T12`), find the first replacement in source order whose leftmost filter name is exactly `type` (the chains that `render/filters#F1` rule 1 rewrites) and whose key is the field name. Sections are not evaluated: a replacement inside any section counts. The legacy names `type-nc` and `type-cloze` do not count here; their markers already carry their flag.
3. That replacement's other filter names that are `cloze`, `nc` or `ci`, in any order and position, are added to the flags. Other names are ignored. When no replacement is found (the marker came from a field's text, or a template writes it literally), the flags stay A4.1's.

Then:

- `cloze`: the expected text is the cloze extraction for the card number, with the cloze warning when it is empty or the field is missing (A4.2, A4.3).
- `nc`: mode nc (A6.3).
- `ci`: case is ignored. A6 runs unchanged except that every character of T and of X is replaced by its **simple case folding** for the equality test (A6.4) and for the matcher (A6.5). Token texts and the display text come from the unfolded sequences, exactly as A6 says. Simple case folding is Unicode 16.0.0 `CaseFolding.txt`, statuses C and S; a character with no such line stays as it is. Folding maps one character to one character, so token positions stay valid.

Folding examples: `Σ`, `σ` and `ς` all fold to `σ`; `K` (U+212A KELVIN SIGN) to `k`; `ſ` to `s`; `ǅ` to `ǆ`; `ß` stays `ß` (its folding to `ss` is a full folding, status F), so `Strasse` does not match `Straße`; `İ` stays `İ`.

| Template | Expected | Typed | Meguri's comparison |
|---|---|---|---|
| `{{type:ci:Answer}}` | `Qavru Vexal` | `qavru vexal` | `<code id=typeans><span class=typeGood>Qavru Vexal</span></code>` |
| `{{type:ci:Answer}}` | `Qavru Vexal` | `qavru vexul` | Good `qavru vex`, Bad `u`, Good `l` \| Good `Qavru Vex`, Missed `a`, Good `l` |
| `{{type:nc:ci:Answer}}`, `{{type:ci:nc:Answer}}` | `Qëvax` | `QEVAX`, `qevax` | Good `Qëvax` |
| `{{type:cloze:nc:Text}}`, card 1 | `{{c1::café}} y {{c1::té}}` | `cafe, te` | Good `café, té` |
| `{{type:ci:cloze:Text}}`, card 1 | `{{c1::Paris}} is big` | `paris` | Good `Paris` |

When typed text already matches exactly, the output is Anki's. Otherwise the answer differs from the oracle's; `conformance/deviations.yaml` records Meguri's answers for `render/type-answer/meguri-modifiers` (entries `type-answer-ci-*` and `type-answer-cloze-*`). With `cloze` added, the question can differ too: Anki shows the box when the whole field is not empty, Meguri when the extraction is not empty.

Anki's main branch has tests of case-insensitive comparison (supplementary harvest rows), so a later oracle may bring its own `ci`. Adopting such a bump must check A9 and its deviation entries against it.

## A10. Reveal

Meguri renders the answer before the user types (U176), so the comparison is computed at reveal. Only C (A5) depends on the typed text: the answer can be built at render time with the comparison left open in every B, together with the type-in (expected text, mode, `ci`, font and size), and at reveal the core computes C from the type-in and the typed text, and the page puts C in every place left open. The result must equal A5 for that typed text, character for character. The rule moves (A5 rule 2) do not depend on the typed text. A card without a type-in needs no call at reveal. How the open places are marked is U67's and U176's choice. The comparison takes microseconds for real answers, within R1809.

## A11. Previews

### A11.1 Template preview (R941; U71 wires it)

The template editor's preview simulates a typed answer. After A3 steps 1 to 3 (no A4, A5 or A9), a side that contains the text `[[type:` anywhere is transformed; any other side is shown unchanged.

- **Both sides**: every `<hr id=answer>` (that exact text) is removed first, whether or not a marker follows.
- **Question**: the first marker (A2) becomes `<center><input id='typeans' type=text value='example' readonly='readonly'></center>`; every later marker becomes `<center><b>` W `</b></center>`. The removed rules are not put back.
- **Answer**: the first marker becomes C0, with `<hr id=answer>` in front when the answer had one; every later marker becomes `<center><b>` W `</b></center>`. C0 is the comparison of expected `example` with typed `sample` in plain mode: `<code id=typeans><span class=typeBad>s</span><span class=typeGood>ample</span><br><span id=typearrow>&darr;</span><br><span class=typeMissed>ex</span><span class=typeGood>ample</span></code>`, with no font `div` around it.
- Nothing depends on the note's fields: no font, no size, no warning of A8.
- W is Meguri's text `Only the first type-in box of a card template works.` Anki's sentence for it is not used: the preview has no oracle, and the similarity scan's text channel would flag it (R1959).

### A11.2 Browse preview (R1220)

The Browse preview shows no type-in: on both sides, before A3 step 2, every match of `[[type:`, one or more characters other than `]` (line feeds allowed), then `]]`, is removed. No box, comparison or warning is shown. The Browse preview's other rules (sides, moving through the results, re-rendering, audio, closing) are `browser/preview`.

## A12. Plan alignment

The U64 approach and test examples now follow A4.3: every question marker shares the first descriptor, and only the first input supplies text. Unknown ordinary template fields remain template errors; literal markers and special-field markers exercise A8. This corrects the former approach wording under R1907, without changing R532's first-input rule. A5 moves every exact answer rule whenever the answer contains it. A9 is Meguri's own modifier behavior.

## A13. Scenarios

| Scenario | Covers |
|---|---|
| `render/type-answer/compare` | A6 and A7 through `type_answer`: AE17's `ka`/`kq`/empty, R941's inputs, every step of A6.1, both modes, NFC and NFKD cases, marks at token starts in both Unicode versions, the matcher's long-text behavior |
| `render/type-answer/corpus/compare-*` | A6 on 3,000 generated pairs (`conformance/corpora/type-answer/generate.py`) |
| `render/type-answer/question` | A2, A4: fonts and sizes from the field config, several boxes, empty and `<br>` fields, the unknown-field warning for a special name and a literal marker, a marker in a field's text |
| `render/type-answer/answer` | A5: rules after, before, twice, quoted, absent, without a marker, formed by the removal; an answer marker naming another field; typed case, spaces, markup and `&`; media in the expected text |
| `render/type-answer/cloze` | A4.2 for cloze type-ins (R977): joined and identical answers, a missing answer, a literal cloze marker, a standard note type's card numbers |
| `render/type-answer/modifiers` | A4.1 and A6 for `nc`: `type:nc:`, `type-nc:`, and the plain marker on the same texts |
| `render/type-answer/meguri-modifiers` | A9, with its deviation entries |
| `render/type-answer/replacement-escapes` | A4.5: valid replacement escapes, capture expansion and typed malformed-input refusals |
| `render/type-answer/whole-side` | A3: CSS and body are transformed together, including media names, repeated markers and answer-rule relocation |
| `render/type-answer/ae17` | AE17's comparison part: a Japanese syllabary card, typed `ka`, `kq` and nothing |
| `render/type-answer/av-removal` (held) | A6.1 step 1 and A6.1.1 on their own: a sound tag between words leaves the spaces on both sides of it, adjacent sound tags, `tts` directives with only a non-`lang` option and with `lang`, a mixed line, and a directive with another name kept with its option (also against a typed text) |

The media-name side of the same review HTML is `render/media-display`; the image-occlusion elements are `render/image-occlusion-render`.

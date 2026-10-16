# Image occlusion: detection and the Occlusion field format

Spec: render/image-occlusion-format
Requirements: R990, R1003, R1004, R1005, R1006, R1007, R1008, R1009, R1010, R1011, R1012, R1013 (data side), R1016 (data side); supports AE33, AE38
Anki: 26.09.3
Scenarios: conformance/scenarios/render/image-occlusion/format/

Which note types are image-occlusion (IO) note types, how their Occlusion field stores masks and text labels, how Anki reads that field, how Meguri reads it for its mask editor and writes it back, and how masks get their cloze numbers. The card HTML built from the field (the shape `div`s) is U64's (`render/image-occlusion-render`); the cloze tree it relies on is `render/cloze`. The observation that exposes the readings to scenarios is `specs/conformance/image-occlusion-ops.md`.

## F1. Terms

- **IO note**: a note whose note type F2 detects. Its **Occlusion field** holds the shapes; its **Image field** holds the `<img>` of the image they cover.
- **Shape**: one item drawn over the image. The kinds are `rect`, `ellipse` and `polygon`, together called **masks**, and `text`, called **text labels**.
- **Number**: the cloze number a shape is stored under (`{{c3::…}}` has number 3). Card N of the note is the card generated for cloze number N (`render/cloze#C1`). Masks that share a number hide and show together on that card. Number 0 never creates a card (`render/cloze#C5`): a shape with number 0 is shown on every card, on both sides. Text labels have number 0 (R1008).
- **Mode**, one per note (R1000, R1007): **Hide All, Guess One** (the other masks are drawn too on each card) or **Hide One, Guess One** (only the card's own masks are drawn).
- **Fractions**: positions and sizes are stored as fractions of the image. Horizontal values are fractions of the image width, vertical values of its height. (0, 0) is the image's top-left corner and (1, 1) its bottom-right corner. Values below 0 or above 1 are allowed: a shape may stick out of the image.
- **Raw reading** (F4, F5): what Anki reads from the field. It never fails. Card rendering uses it (U64), and the oracle observes it.
- **Strict reading** (F7): Meguri's reading for its mask editor. It gives typed shapes, or the reason it cannot (R1016).
- **Writer** (F8–F11): Meguri's writer of the field. Anki writes this field only in its web mask editor, which the oracle cannot run, so the writer's output is defined by this spec alone. The oracle checks that Anki reads it as intended (F16, `written`).

## F2. Detecting an IO note type (R990)

- **Field tags.** A field's config may carry `tag`, an optional unsigned 32-bit integer (protobuf field 10 of the field config, `model/protobuf-records`; schema-11 key `tag`, `model/schema11-json#S5`). The IO tags are 0 Occlusion, 1 Image, 2 Header, 3 Back Extra and 4 Comments. Other values, and fields without a tag, have no meaning here.
- **Field roles.** The four roles Occlusion, Image, Header and Back Extra are found as follows:
  1. When each of the tags 0, 1, 2 and 3 is carried by at least one field, each role is the lowest-index field that carries its tag.
  2. Otherwise, when the note type has at least 4 fields, the roles are fields 0, 1, 2 and 3, in that order.
  3. Otherwise the note type has no field roles.

  A partial set of tags counts for nothing: when one of the four tags is missing, rule 2 applies even where it disagrees with the tags that are present. Tag 4 never matters, and neither do field names.
- **Detection.** A note type is an IO note type when its `original_stock_kind` (protobuf field 9 of the note type config; schema-11 `originalStockKind`) is 6, image occlusion, and it has field roles. Its kind (cloze or standard) is not checked. A note type with stock kind 6 and three fields is not an IO note type, and neither is a note type with four fields and another stock kind, although it has field roles.

Examples, the fields' tags in field order (`-` for no tag):

| Tags | Roles: Occlusion, Image, Header, Back Extra |
|---|---|
| `0 1 2 3` | 0, 1, 2, 3 |
| `3 2 1 0` | 3, 2, 1, 0 |
| `4 0 1 2 3` | 1, 2, 3, 4 |
| `- 0 1 2 3` | 1, 2, 3, 4 |
| `1 0 - 2 3` | 1, 0, 3, 4 |
| `0 0 1 2 3` | 0, 2, 3, 4 (the first field with tag 0) |
| `3 1 - 2 0` | 4, 1, 3, 0 |
| `- 0 1 2` | 0, 1, 2, 3 (tag 3 is missing, so rule 2) |
| `0 1 2 -` | 0, 1, 2, 3 |
| `- - - -`, `7 7 7 7`, `- - - - - -` | 0, 1, 2, 3 |
| `0 1 2`, `- - -` | none |

## F3. The Occlusion field

- The field holds one IO cloze per shape. Anki and Meguri write each one followed by `<br>` (F11):
  `{{c1::image-occlusion:rect:left=.2325:top=.3261:width=.202:height=.0975:oi=1}}<br>{{c2::image-occlusion:ellipse:left=.1:top=.2:rx=.05:ry=.08:oi=1}}<br>`
- The field is read as a cloze tree (`render/cloze#C2`–`C4`). An **IO cloze** is a top-level cloze whose first child is a text that starts with `image-occlusion:` (case-sensitive; `render/cloze#C11`). A cloze inside another cloze is never a shape, even when it starts with the prefix.
- The **shape text** of an IO cloze is its first child text after the prefix. That text run ends at the cloze's closing `}}` or at the opening marker of a nested cloze (`render/cloze#C3`). Hint splitting never applies to it (`render/cloze#C4`), so `::` inside it is ordinary text. The format has no escape for `}`: a `}}` always ends the cloze.
- The **source** of an IO cloze is the run of field characters from the first `{` of its opening marker to the last `}` of its closing marker.

## F4. Raw reading of a shape text

A shape text S gives either no shape, or a shape name and a list of properties (key, value), in the order they appear. Repeated keys are all kept.

1. When S contains no `:`, there is no shape.
2. The shape name is S up to its first `:`. It may be empty and may be any text.
3. Properties are read one after another, starting at that first `:`. A property is:
   - a `:`;
   - its **key**: the characters after that `:` up to the next `=`. The key has at least one character. It may contain `:` and `\` and is taken exactly as written: keys are never unescaped;
   - an `=`;
   - its **value**, read from left to right: `\\` stands for `\`, `\:` stands for `:`, and any other character except `\` and `:` stands for itself. The value ends at the first `:` that is not part of `\:`, or at the end of S.
4. Reading stops at the end of S, or at the first property that cannot be read. The properties read before that point are the shape's; the rest of S is ignored. A property cannot be read when:
   - its key is empty, or no `=` follows the key before the end of S;
   - its value contains a `\` followed by any character other than `\` and `:`, or ends S with an unpaired `\`;
   - its value is empty and a `:` follows it. An empty value is read only at the very end of S.

| Shape text | Name | Properties |
|---|---|---|
| `rect:left=.2:top=.3` | `rect` | `left` `.2`, `top` `.3` |
| `text:text=a\:b\\c:left=1` | `text` | `text` `a:b\c`, `left` `1` |
| `rect:a=\:\::b=1` | `rect` | `a` `::`, `b` `1` |
| `rect:left=` | `rect` | `left` (empty) |
| `rect:a=1:b=` | `rect` | `a` `1`, `b` (empty) |
| `rect:left=:top=1` | `rect` | none: an empty value followed by `:` |
| `rect:left=1\x:top=2` | `rect` | none: `\x` |
| `rect:left=1\` | `rect` | none: unpaired `\` at the end |
| `rect:left=a:` | `rect` | `left` `a` (the last `:` starts no property) |
| `rect:a=1:b` | `rect` | `a` `1` (no `=` after `b`) |
| `rect:` , `rect::` | `rect` | none |
| `rect:foo:left=1:top=2` | `rect` | `foo:left` `1`, `top` `2` |
| `rect:a=1::b=2` | `rect` | `a` `1`, `:b` `2` |
| `rect::=1` | `rect` | `:` `1` |
| `rect:a\=1:b=2` | `rect` | `a\` `1`, `b` `2` |
| `rect:a=b=c` | `rect` | `a` `b=c` |
| `rect:left=1:left=2` | `rect` | `left` `1`, `left` `2` |
| `rect: left= 1 ` | `rect` | ` left` ` 1 ` (spaces kept) |
| `rect:a={x}y<br>` | `rect` | `a` `{x}y<br>` |
| `:left=1` | (empty) | `left` `1` |
| `star:left=1`, `Rect:left=1` | `star`, `Rect` | `left` `1` |
| `image-occlusion:rect:a=1` | `image-occlusion` | `rect:a` `1` |
| `rect`, (empty) | no shape | |

## F5. Raw reading of a field (R1003, R1007)

- Every IO cloze whose shape text gives a shape (F4) contributes that shape under each number of its number list (`render/cloze#C2`), 0 included. `{{c1,2::image-occlusion:rect:…}}` is listed under 1 and under 2.
- The result lists, in ascending order, the numbers that have at least one shape, and under each number its shapes in field order. An IO cloze that gives no shape adds nothing, not even its number.
- **Mode.** The field is in Hide All mode when at least one listed shape has `1` as the value of its *first* property keyed `oi` (key exactly `oi`, value exactly `1`). Otherwise it is in Hide One mode. Later `oi` properties of a shape do not count: `oi=0:oi=1` is not Hide All, and neither are `oi=1 ` (trailing space) and `oi=true`. A field with no shape is in Hide One mode.
- Nothing else is checked: unknown shape names, unknown keys and any value text are listed as read.

| Field | Numbers: shapes | Mode |
|---|---|---|
| `{{c3::image-occlusion:rect:a=1}}<br>{{c1::image-occlusion:rect:a=2}}<br>{{c3::image-occlusion:ellipse:a=3}}<br>` | 1: rect `a=2`; 3: rect `a=1`, ellipse `a=3` | Hide One |
| `{{c1,2::image-occlusion:rect:left=1}}{{c2::image-occlusion:ellipse:left=2}}{{c0::image-occlusion:text:text=x}}` | 0: text; 1: rect; 2: rect, ellipse | Hide One |
| `{{c0::image-occlusion:text:text=t:oi=1}}{{c1::image-occlusion:rect:a=1}}` | 0: text; 1: rect | Hide All (the label's `oi` counts) |
| `{{c1::image-occlusion:rect:oi=0:oi=1}}` | 1: rect | Hide One |
| `{{c1::image-occlusion:rect}}` | none | Hide One |
| `{{c2::x {{c1::image-occlusion:rect:left=1}}}}` | none (not top-level) | Hide One |
| `{{c1::image-occlusion:rect:a=1{{c2::y}}z}}` | 1: rect `a=1` | Hide One |
| `{{c1::image-occlusion:rect:a=1` (never closed) | none | Hide One |
| `{{c1:: image-occlusion:rect:left=1}}` | none (a space before the prefix) | Hide One |
| `{{c1::image-occlusion:text:text=a}}b:scale=1}}` | 1: text `text=a` | Hide One |
| `{{c1::image-occlusion:rect:left=1::hint}}` | 1: rect `left=1` | Hide One |
| `{{c01::image-occlusion:rect:a=1}}` | 1: rect `a=1` | Hide One |

## F6. Shape kinds and keys

Keys every kind knows:

| Key | Meaning | When absent |
|---|---|---|
| `left`, `top` | fractions: the shape's top-left corner before rotation (for an ellipse, the corner of its bounding box; for a polygon, see below) | (required, F7) |
| `angle` | rotation in steps of 1/10 000 of a full turn (F9), clockwise on screen (y grows downward), about the point (`left`, `top`) | 0 |
| `fill` | the fill color, a CSS color text as written (the editors write `#rrggbb` in lower case) | `#ffeba2` for masks, `#000000` for text labels |
| `oi` | `1` marks Hide All mode (F5) | the shape does not mark Hide All |

Keys of each kind:

| Kind | Keys | Meaning |
|---|---|---|
| `rect` | `width`, `height` | the size, fractions of the image width and height |
| `ellipse` | `rx`, `ry` | the radii, `rx` a fraction of the image width and `ry` of its height; the center is (`left` + `rx`, `top` + `ry`) |
| `polygon` | `points` | the corners: `x,y` pairs separated by single spaces, fractions. The polygon is drawn at its points moved by (`left` − smallest x, `top` − smallest y), so a polygon whose `left` and `top` are its points' minimums is drawn exactly at its points. Anki's editor may store `left` and `top` slightly off those minimums (by about half its border width); reading keeps them as stored. Anki's reviewer ignores `angle` on polygons: rotating a polygon in an editor moves its points instead. |
| `text` | `text`, `scale`, `fs` | `text` is the label (a line feed breaks the line); `scale` its uniform scale factor (absent: 1); `fs` its font size before scaling, a fraction of the image height. Labels written by Anki before 24.06 have no `fs`; they are drawn at 40 pixels of the image as displayed (U64, U205). |

A key known to one kind is unknown to the others: `width` on an ellipse or `text` on a rect is an unknown key there (F7.3).

## F7. Strict reading for the mask editor (R1013, R1016)

Input: the Occlusion field's text. Output: the shapes in field order with the mode, or one error. Meguri's mask editor opens a note only when this succeeds; otherwise it reports the error, leaves the note unchanged and offers the normal field editor (R1016, U205).

### F7.1 Value forms

**Number.** An optional sign (`+` or `-`); then a mantissa, which is ASCII digits, digits `.` digits, digits `.`, `.` digits, or `.` alone; then optionally `e` or `E`, an optional sign and one or more digits, allowed only when the mantissa has at least one digit. Nothing else is part of a number: no white space, no `_`, no `Infinity`, `NaN` or hexadecimal. The number's value is the decimal's value rounded to the nearest 64-bit float (ties to even). A mantissa without a digit (`.`, `-.`, `+.`) has the value 0. A number whose rounding overflows (`1e400`) is not valid.

| Valid | Value |
|---|---|
| `.2081`, `0.2081`, `+.2081` | the same value |
| `1.`, `1`, `1.0000` | 1 |
| `-0.25` | −0.25 |
| `.`, `-0.`, `.0000` | 0 |
| `1e-3`, `.5E2` | 0.001, 50 |

Not valid: `abc`, empty, ` 1`, `1 `, `1,5`, `0x10`, `inf`, `NaN`, `1e`, `.e5`, `e5`, `1e400`, `--1`.

**Points.** One or more pairs separated by single spaces (U+0020). A pair is a number, `,`, a number. Not valid: an empty value, a leading or trailing space, two spaces in a row, a pair without a comma or with two.

### F7.2 Checks and errors

The checks run in this order; the first failure is the error.

1. **`extra_content`**: some character of the field belongs neither to the source of a top-level IO cloze (F3) nor to a **separator**, a top-level text made only of spaces, tabs, line feeds, carriage returns, form feeds and the tags `<br>`, `<br/>` and `<br />` (the letters `br` in any case). This covers any other text, a cloze that is not an IO cloze, an IO cloze inside another cloze, a stray `}}`, and an opening marker that is never closed (`render/cloze#C4` drops its cloze).
2. Then each top-level IO cloze, in field order. The first one that fails a check gives the error, and the checks run on it in this order:
   1. **`multiple_numbers`**: its number list has more than one number (`{{c1,2::…}}`).
   2. **`nested_cloze`**: it holds a nested cloze.
   3. **`malformed_shape`**: its shape text gives no shape (F4 step 1), or F4's reading stops before the end of the shape text.
   4. **`unknown_shape`**: the shape name is not exactly `rect`, `ellipse`, `polygon` or `text`.
   5. **`duplicate_key`**: two of its properties have the same key.
   6. **`invalid_value`**: a key known to its kind (F6) has a value of the wrong form. `left`, `top`, `angle`, `width`, `height`, `rx`, `ry`, `scale` and `fs` take numbers; `points` takes points. `text`, `fill` and `oi` take any value, the empty one included.
   7. **`missing_key`**: `left` or `top` is missing, or the kind's own required keys are: `width` and `height` for `rect`, `rx` and `ry` for `ellipse`, `points` for `polygon`, `text` for `text`.

The error is `{kind, shape}`: `shape` is the IO cloze's index in field order (from 0), or none for `extra_content`.

An empty field, or one made only of separators, reads as no shapes in Hide One mode.

### F7.3 Typed shapes

Each shape holds:

- its kind and its number (the single number of its IO cloze);
- `left` and `top`, and the kind's values: `width` and `height`; `rx` and `ry`; the points as (x, y) pairs; or `text`, `scale` (1 when absent) and `fs` (none when absent);
- its angle in degrees, read from `angle` by F9 (0 when absent);
- its fill: the `fill` value, or none when it is absent or empty;
- whether its `oi` value is `1`;
- its **unknown properties**: those whose keys its kind does not know (F6), with their values, in field order;
- its source (F3).

The mode is F5's. Since keys never repeat after F7.2, "the first `oi`" is the shape's only `oi`.

### F7.4 Groups

Masks that share a number of 1 or more form a group when there are at least two of them; the editor shows a group as one object (R1013). Masks with number 0 and text labels never belong to a group, whatever their number.

### F7.5 Examples

| Field | Result |
|---|---|
| `{{c1::image-occlusion:rect:left=.2:top=.3:width=.1:height=.1:oi=1}}<br>` | one rect, number 1, Hide All |
| `{{c1::image-occlusion:rect:left=0.2081:top=.3:width=.1:height=.1}}` and the same with `.2081` | equal shapes |
| `{{c1::image-occlusion:rect:left=abc}}` | `invalid_value`, shape 0 |
| `{{c1::image-occlusion:rect:left=.1:top=.1:width=.1}}` | `missing_key`, shape 0 |
| `{{c1::image-occlusion:rect:left=.1:top=.1:width=.1:height=.1}}<br>{{c2::image-occlusion:rect}}` | `malformed_shape`, shape 1 |
| `{{c1::image-occlusion:star:left=.1:top=.1}}` | `unknown_shape`, shape 0 |
| `{{c1,2::image-occlusion:rect:left=.1:top=.1:width=.1:height=.1}}` | `multiple_numbers`, shape 0 |
| `{{c1::image-occlusion:rect:left=.1:left=.2:top=.1:width=.1:height=.1}}` | `duplicate_key`, shape 0 |
| `{{c1::image-occlusion:rect:left=.1:top=.1:width=.1:height=.1{{c2::x}}}}` | `nested_cloze`, shape 0 |
| `{{c1::x}}`, or a valid field followed by `junk`, or by `{{c3::image-occlusion:rect:left=.1` | `extra_content` |
| `{{c0::image-occlusion:text:left=.1:top=.1:text=}}` | one text label with empty text, `scale` 1, no `fs` |
| `{{c1::image-occlusion:rect:left=.1:top=.1:width=.1:height=.1:zz=q\:r}}<br />` | one rect with the unknown property `zz` = `q:r` |
| `{{c0::image-occlusion:rect:left=.:top=.:width=.5:height=.5}}<BR>` | one mask with number 0, `left` and `top` 0 |

## F8. Writing numbers (R1005)

Every number the writer stores, except `angle` (F9), is written from a finite 64-bit float x as follows:

1. When x is zero (+0 or −0), the text is `.0000`.
2. Otherwise, let n be the integer nearest to |x| × 10 000, computed on the exact value of x without intermediate rounding. When |x| × 10 000 lies exactly halfway between two integers, n is the larger one. Write n in decimal, padded on the left with zeros to at least five digits, and put a `.` before its last four digits. Put `-` in front when x is negative.
3. Remove every `0` at the very start of that text, then every `0` at its very end.

| x | Text |
|---|---|
| 0, −0 | `.0000` |
| 0.2081, 0.20814 | `.2081` |
| 0.20815 (stored as 0.2081500000000000016…) | `.2082` |
| 12.34565 (stored as 12.3456499999999991…) | `12.3456` |
| 0.03125 (exactly halfway) | `.0313` |
| 0.96875 | `.9688` |
| 0.5 | `.5` |
| 0.202 | `.202` |
| 1, 0.99995 | `1.` |
| 10 | `10.` |
| 1.5 | `1.5` |
| 0.00004, 1e-14 | `.` |
| 0.00005 | `.0001` |
| −0.25 | `-0.25` (a leading `-` protects the `0`) |
| −1 | `-1.` |
| −0.00001, −1e-14 | `-0.` |

A rounding that sends exact halves to even (0.03125 → `0.0312`) gives the wrong text, as does rounding a decimal approximation instead of the exact value. The text `.` and `-0.` are what Anki writes for tiny nonzero values; F7.1 reads them as 0.

## F9. Angles (R1006)

**Writing** an angle of d degrees (finite):

1. a = the remainder of d divided by 360, with the sign of d (floating-point `fmod`, which is exact).
2. r = (a / 360) × 10 000, in 64-bit floating point, the division first, each operation rounded to nearest (ties to even).
3. s = r rounded to an integer, halves toward positive infinity (4999.5 → 5000, −4999.5 → −4999).
4. When s is 0 (or −0), no `angle` key is written. Otherwise `angle=` and s in decimal, with `-` when negative.

| Degrees | Stored |
|---|---|
| 0, 360, 720, 0.01, −0.01 | (no key) |
| 90, 450 | `2500` |
| 45 | `1250` |
| 30 | `833` |
| 1 | `28` |
| 12.5 | `347` |
| 180 | `5000` |
| 359.99 | `10000` |
| −90 | `-2500` |
| 179.982 (r = 4999.5) | `5000` |
| −179.982 (r = −4999.5) | `-4999` |
| 539.982 (a = 179.98199999999997, r = 4999.499999999999) | `4999` |

**Reading** a stored number v (F7.1) gives ((v mod 10 000) / 10 000) × 360 degrees, the remainder taken with the sign of v and the operations done in 64-bit floating point in that order: 2500 → 90, 10 000 → 0, 12 500 → 90, −2500 → −90, 833 → 29.988. Writing a value read this way gives back the same `s` for every integer v from −9999 to 9999; ±10 000 read as 0° and are then written as no key.

## F10. Writing a shape (R1003, R1004, R1006, R1007, R1008)

A shape is written as `{{c` N `::image-occlusion:` kind, then each property as `:` key `=` value, then `}}`.

- **N**: the shape's number written in decimal; for a text label always `0` (R1008).
- **Values** are escaped: each `\` becomes `\\` and each `:` becomes `\:`. Nothing else is escaped. Keys are written as they are.
- **Keys, in this order** (R1004):

  | Kind | Order |
  |---|---|
  | `rect` | `left`, `top`, `angle`\*, `width`, `height`, `fill`\*, unknown properties, `oi`\* |
  | `ellipse` | `left`, `top`, `angle`\*, `rx`, `ry`, `fill`\*, unknown properties, `oi`\* |
  | `polygon` | `left`, `top`, `angle`\*, `points`, `fill`\*, unknown properties, `oi`\* |
  | `text` | `left`, `top`, `angle`\*, `text`, `scale`, `fs`\*, `fill`\*, unknown properties, `oi`\* |

  \* `angle` only when F9 stores a nonzero value; `fs` only when the label has one; `fill` only when the shape has one and it differs from its kind's default (`#ffeba2` for masks, `#000000` for text labels; exact comparison, so `#FFEBA2` is written); `oi` with the value `1` only in Hide All mode (R1007), for every shape, text labels included.
- **Value texts**: `left`, `top`, `width`, `height`, `rx`, `ry`, `scale` and `fs` by F8; `points` as each point's x and y by F8 joined by `,`, the points joined by single spaces; `angle` by F9; `text` and `fill` as they are; unknown properties with the values read (F7.3), in their order. An unknown property whose value is empty is left out: F4 cannot read an empty value that another property follows.
- **Refusal.** The writer refuses a shape, with the error `unwritable_shape` and the shape's index in its input, when a number to write is not finite, or when the written text, read back by F3–F5, is not one top-level IO cloze with the same number, kind and property list. A text label holding `}}`, or a value ending in `}` written last before the closing `}}`, is refused this way: the format cannot store it (F3). U205 keeps such labels out of the editor.

| Shape | Mode | Written |
|---|---|---|
| rect, number 1, left 0.2325, top 0.3261, width 0.202, height 0.0975 | Hide All | `{{c1::image-occlusion:rect:left=.2325:top=.3261:width=.202:height=.0975:oi=1}}` |
| the same rect rotated 90° | Hide All | `{{c1::image-occlusion:rect:left=.2325:top=.3261:angle=2500:width=.202:height=.0975:oi=1}}` |
| ellipse, number 2, left 0.1, top 0.2, rx 0.05, ry 0.08, fill `#ff0000` | Hide One | `{{c2::image-occlusion:ellipse:left=.1:top=.2:rx=.05:ry=.08:fill=#ff0000}}` |
| polygon, number 3, left 0.3, top 0.4, points (0.3, 0.45) (0.5, 0.4) (0.35, 0.6) | Hide All | `{{c3::image-occlusion:polygon:left=.3:top=.4:points=.3,.45 .5,.4 .35,.6:oi=1}}` |
| text `a:b\c`, left 0.1, top 0.1, scale 1, fs 0.0666667 | Hide All | `{{c0::image-occlusion:text:left=.1:top=.1:text=a\:b\\c:scale=1.:fs=.0667:oi=1}}` |
| text `Label`, left 0.5, top 0.25, 45°, scale 1.5, fs 0.05, fill `#1e90ff` | Hide One | `{{c0::image-occlusion:text:left=.5:top=.25:angle=1250:text=Label:scale=1.5:fs=.05:fill=#1e90ff}}` |
| text `line 1` LF `line 2`, left 0.2, top 0.7, scale 0.75, fs 0.04 | Hide One | `{{c0::image-occlusion:text:left=.2:top=.7:text=line 1` LF `line 2:scale=.75:fs=.04}}` |
| text `日本:語`, left 0.6, top 0.6, scale 1, fs 0.05 | Hide All | `{{c0::image-occlusion:text:left=.6:top=.6:text=日本\:語:scale=1.:fs=.05:oi=1}}` |
| rect, number 4, left 0, top 1, width 1, height 0.5, −90° | Hide One | `{{c4::image-occlusion:rect:left=.0000:top=1.:angle=-2500:width=1.:height=.5}}` |
| rect, number 5, left 0.00004, top −0.00001, width 0.03125, height 0.96875 | Hide One | `{{c5::image-occlusion:rect:left=.:top=-0.:width=.0313:height=.9688}}` |
| rect, number 6, read from `{{c6::image-occlusion:rect:zz=q\:r:left=.1:top=.1:width=.1:height=.1}}`, moved to left 0.2 | Hide All | `{{c6::image-occlusion:rect:left=.2:top=.1:width=.1:height=.1:zz=q\:r:oi=1}}` |
| text label read with number 3, then moved | Hide One | written with `{{c0::` |
| rect, fill `#ffeba2` given explicitly | Hide One | no `fill` key |
| text `a}}b` | either | refused (`unwritable_shape`) |

## F11. Writing the field (R1011, R1012; AE38)

The writer's input: the shapes in the editor's order (bottom to top; a group's masks one after another), the mode, and the **reference size** W × H, in pixels, at which the editor shows the whole image (fit to its area, as U205 lays it out). W and H must be positive and finite; otherwise the writer refuses the whole input with the error `invalid_size`. A shape that came from the field carries the source it was read from (F7.3).

1. **Unchanged save.** When the input's shapes are exactly the shapes the strict reading gave (same count and order, each equal to what was read) and the mode is the mode read, the output is the original field text, byte for byte, separators included (R1012).
2. Otherwise each shape, in input order, gives one of:
   1. **its source**, when it has one, it is equal to what was read from that source, and whether its `oi` value is `1` matches the mode being written. Untouched masks thus stay byte-identical (R1012, AE38), whatever layout or number forms they use (F14);
   2. **nothing** (dropped, R1011), when it is not written from its source and:
      - its fill is exactly `transparent`; or
      - it is a text label with empty text; or
      - it is a rect with width × W < 5 or height × H < 5; an ellipse with 2 × rx × W < 5 or 2 × ry × H < 5; or a polygon whose points span less than 5 pixels: (largest x − smallest x) × W < 5 or (largest y − smallest y) × H < 5;
   3. **its F10 text** otherwise.

   Each source or F10 text is followed by `<br>`, and the output is their concatenation. When nothing is left, the output is the empty string.
3. **Equal** means: the same kind and number, the same numbers compared as values (0 and −0 are equal), the same texts, the same points, the same unknown properties in the same order.

Untouched shapes are never dropped by rule 2.2, even when they are smaller than 5 pixels at this reference size: R1012 keeps them. Separators between untouched shapes are not kept once anything changes: every shape is followed by exactly one `<br>`.

**Example (AE38).** A field in Hide All mode with seven masks; c2 uses an older layout (F14):

```
{{c1::image-occlusion:rect:left=.05:top=.1:width=.2:height=.08:oi=1}}<br>
{{c2::image-occlusion:rect:left=0.3:top=0.1:fill=#ffeba2:width=0.2:height=0.08:oi=1}}<br>
{{c3::image-occlusion:ellipse:left=.55:top=.1:rx=.1:ry=.04:oi=1}}<br>
{{c4::image-occlusion:rect:left=.05:top=.3:width=.2:height=.08:oi=1}}<br>
{{c5::image-occlusion:polygon:left=.3:top=.3:points=.3,.3 .5,.32 .4,.4:oi=1}}<br>
{{c6::image-occlusion:rect:left=.55:top=.3:width=.2:height=.08:oi=1}}<br>
{{c7::image-occlusion:rect:left=.05:top=.5:angle=833:width=.2:height=.08:fill=#cc3344:oi=1}}<br>
```

(one line in the field; shown here on seven). The user moves c4 to left 0.0625, top 0.35 and deletes c6. The writer outputs the c1, c2, c3, c5 and c7 lines exactly as above, with the c4 line replaced by `{{c4::image-occlusion:rect:left=.0625:top=.35:width=.2:height=.08:oi=1}}<br>` and the c6 line gone. Saving with nothing changed returns the seven lines unchanged. Switching to Hide One without moving anything rewrites every mask without `oi`: c2 becomes `{{c2::image-occlusion:rect:left=.3:top=.1:width=.2:height=.08}}<br>`. Card 6 is not deleted by the save (R1015); U205 warns about it.

## F12. Numbers for new and edited shapes (R1009, R1010)

Pure rules over the shapes currently in the editor. U205 applies them; nothing else ever changes a number.

- **Used numbers**: the numbers of all shapes in the editor, text labels included.
- **New mask**: the smallest integer of 1 or more that is not used. Numbers used only by other fields of the note (an `A06` audio cloze, R1018) do not count, and neither do cards left behind by deleted masks: a new mask can take a deleted mask's number and its card.
- **New text label**: 0.
- **Duplicate**: the copy is new. A copied mask gets a new number; a copied group gets one new number for all its masks; copied text labels get 0.
- **Group** (two or more selected masks): every selected mask takes the smallest number among the selected masks. Selected text labels are left out and keep 0. A selection with fewer than two masks changes nothing.
- **Ungroup** (masks sharing number m, in the editor's order): the first keeps m; each following mask, in order, takes the smallest number of 1 or more not used at that moment, counting the numbers this ungroup has already given.
- **Move, resize, rotate, recolor, edit a label's text, reorder, delete**: no number changes. Gaps left by deleted masks are not closed. Undo and redo restore earlier states, numbers included (R1010).

| Shapes before | Action | After |
|---|---|---|
| c1, c2, c4 | add a mask | the new mask is c3 |
| c2, c3 | add a mask | c1 |
| c1, c2, c3 | add a text label | the label is c0 |
| c1, c2, c3 (new), c4 | group c2 and c4 | both c2 |
| c1, c2, c2 (was c4), c3 | ungroup the c2 pair | the first stays c2, the second becomes c4 |
| c1, c2, c2, c2 (a group of A, B, C), c3 | ungroup | A c2, B c4, C c5 |
| c1 to c4 | duplicate c2 | the copy is c5 |
| c1, c2, c2 (a group), c3 | duplicate the group | the copies are c4, c4 |
| c1 to c7 | move c4, delete c6 | c1 to c5 and c7 keep their numbers |
| c1, c2, c3 | delete c2, add a mask | the new mask is c2 |

## F13. Note-level operations

- **Detect** (note type): the field roles of an IO note type, or none (F2).
- **Get** (note): for a note whose note type is detected, the field roles, the image name of its Image field, and the strict reading of its Occlusion field (F7), shapes or error. A note of any other note type gives the error `not_image_occlusion`.
  - **Image name**: found in the first `<img` tag (lower case, followed by white space) that contains `src` `=` `"` (white space allowed around the `=`) before the tag's first `>`. When that tag contains several, the last counts. The name is the text from that `"` to the next `"`, at least one character, used as written with no decoding (`data-src="…"` also matches). None when no tag has one. Anki then treats the file as the note's image when its extension (after the last `.`, any case) is `jpg`, `jpeg`, `png`, `gif`, `svg`, `webp`, `ico` or `avif` and the file exists in the media folder (U205, U31).
- **Update** (note, Occlusion text): sets the Occlusion field (by its role) to the writer's output and saves the note through the normal note update (U27), which changes nothing when the text is the stored one. A note whose note type is not detected gives `not_image_occlusion`.
- The FFI exposes Detect, the strict reading (F7), the writer (F10, F11) and the number rules (F12). Messages and warnings are U205's (R1015, R1019).

## F14. Layouts in existing collections

Anki's editor has written the field in several layouts. The readings accept all of them; the writer writes only F10's.

| Anki releases | Layout |
|---|---|
| before 24.06 | `fill` right after `top`, before the kind's keys; no `angle`; text labels without `fs` |
| 24.06 to 25.02 | as above, text labels with `fs` |
| 25.07 to 25.09 | `angle` after `top` when nonzero, `fill` after the kind's keys (F10's order) |
| up to 25.09 | only the first `:` of a value escaped, and `\` never escaped |
| 26.09 | F10's order; every `\` and `:` escaped |

Consequences of the old escaping: a label with two colons, `a:b:c`, was stored as `text=a\:b:c:…`, which reads as `text` `a:b` plus an unknown key starting with `c:`; a label with a `\` usually holds an invalid escape, so F4 stops there and F7 reports `malformed_shape`. Anki 23.10 and 23.12 renumbered the masks on every save (their position in the editor plus one); later releases keep numbers. Stored numbers are read as they are.

## F15. Where Meguri's editor differs from Anki's

None of these is visible to the oracle, so `conformance/deviations.yaml` has no entry for them; Meguri's unit tests assert them.

- **Strict reading** (R1016). Anki's editor loads any field, silently skipping unknown shapes and turning unreadable numbers into 0 or nothing, and a save then rewrites the field from what it loaded. Meguri refuses such fields (F7.2) so that no save can lose data. The plan's example `{{c1::image-occlusion:rect:left=abc}}` is `invalid_value`; its raw reading (F4) still lists `left` `abc`.
- **Untouched shapes** (R1012). Anki rewrites every shape on every save, in the order its loader produced (which is not the field order), and drops shapes that are below 5 pixels at the current window size. Meguri keeps untouched shapes' sources and order, and applies the drop rules only to the shapes it writes (F11).
- **Numbers** (R1009, R1010). Anki's editor renumbers polygons it modifies and gives ungrouped masks numbers only at save time; Meguri's numbers change only by F12.
- **Unknown properties** are kept (F7.3, F10); Anki drops them.

## F16. Scenarios

All use the `image_occlusion` observation (`specs/conformance/image-occlusion-ops.md`) except `cards`, which adds notes and observes their cards.

| Scenario | Covers |
|---|---|
| `render/image-occlusion/format/parse-basic` | F5 on fields in Anki's layouts: every kind, groups, text labels, both modes (R1003, R1007) |
| `render/image-occlusion/format/parse-escapes` | F4 escapes and key text (R1003) |
| `render/image-occlusion/format/parse-numbers` | number texts the raw reading keeps as written (R1005) |
| `render/image-occlusion/format/parse-malformed` | F4 stopping rules, unknown names, repeated keys |
| `render/image-occlusion/format/parse-structure` | F3 and F5: number lists, number 0, nesting, unclosed and non-IO clozes, the first-`oi` rule |
| `render/image-occlusion/format/written` | F10 and F11 outputs, the AE38 example included, read back by the oracle |
| `render/image-occlusion/format/fields` | F2 field roles (R990) |
| `render/image-occlusion/format/cards` | the cards an IO field creates: text labels make none (AE33, R1008) |

The strict reading (F7), the writer (F8–F11) and the number rules (F12) have no oracle counterpart. Their tests take expected values from this spec's tables. The writer's outputs above are also inputs of `written`, so the oracle confirms that Anki reads each one as intended. The T3 private suite (R1942) reads every Occlusion field of the user's IO notes strictly and writes it back unchanged; a field that F7 refuses is left unchanged and reported with its error kind.

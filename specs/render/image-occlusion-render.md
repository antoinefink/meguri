# Image occlusion: shape elements in card HTML

Spec: render/image-occlusion-render
Requirements: R539 (the inputs the drawing reads), R1020 and R545 (core side: no audio on the front, only the active mask's audio on the back); verifies AE16 (core part)
Anki: 26.09.3
Scenarios: conformance/scenarios/render/image-occlusion/render/

What the `cloze:` filter writes for an image-occlusion cloze: one empty `div` per shape, whose class and data attributes the card's occlusion script reads to draw the masks (R539, U68). The cloze tree, the activity of clozes and the class rule are `render/cloze#C4`, `#C6` and `#C11`; the shape text and its reading are `render/image-occlusion-format#F3` and `#F4`. Every output here equals the oracle's string for string.

## I1. The element

For an image-occlusion cloze (`render/cloze#C11`) with number list L (`render/cloze#C1`), on a given card and side, the `cloze:` filter writes:

`<div class="` CLASS `" data-ordinal="` L `" ` ATTRIBUTES `></div>`

- CLASS by the first row that applies (`render/cloze#C11`):

  | Condition | CLASS |
  |---|---|
  | L contains 0, or the side is the front and the cloze is active | `cloze` |
  | the cloze is not active | `cloze-inactive` |
  | the side is the back and the cloze is active | `cloze-highlight` |

- ATTRIBUTES (I2) is empty or a list of attributes each followed by one space, so the element always has a space before `>`: `<div class="cloze" data-ordinal="1" ></div>` when there are none.
- The cloze's other children (text after a nested cloze, nested clozes) are not written.

## I2. Attributes

ATTRIBUTES come from the raw reading of the cloze's shape text (`render/image-occlusion-format#F4`): a shape name and its properties in the order read, values unescaped, repeated keys kept.

1. When the shape text gives no shape (it has no `:`), ATTRIBUTES is empty.
2. When the shape name is exactly `rect`, `ellipse`, `polygon` or `text`, ATTRIBUTES starts with `data-shape="` name `" `. Any other name (`star`, `Rect`, the empty name) writes no `data-shape`, and the properties below are still written.
3. Then each property in order, by its key; a key not in the table writes nothing:

   | Key | Written when the value is | Attribute |
   |---|---|---|
   | `left`, `top`, `angle`, `fill` | not empty | `data-` key `="` value `"` |
   | `width`, `height`, `rx`, `ry`, `scale` | neither empty nor exactly `0` | `data-` key `="` value `"` |
   | `oi` | not empty | `data-occludeInactive="` value `"` |
   | `fs` | not empty | `data-font-size="` value `"` |
   | `text` | not empty | `data-text="` E(value) `"`, E the attribute encoding of `render/cloze#C7` |
   | `points` | not empty, and the rewrite below is not empty | `data-points="` rewrite `"` |

   Each attribute is followed by one space.
4. **Values are written as read**, with no escaping, except `text`. Keys belong to no kind: `width` on an ellipse or `points` on a rect is written like any other. A repeated key writes one attribute per occurrence. "Exactly `0`" is the text `0`: `0.0`, `00`, `.0000` and `-0` are written.
5. **Points rewrite.** Split the value at every single space (U+0020; two spaces in a row give an empty piece). Keep each piece that contains a `,`, as the text before its first `,`, a `,`, and everything after that first `,`. Join the kept pieces with single spaces. Pieces without a `,` (including empty ones) are dropped.

## I3. Examples

Recorded from the oracle (`render/image-occlusion/render/divs`); each line is one shape text and its ATTRIBUTES.

| Shape text | ATTRIBUTES |
|---|---|
| `rect:left=.1:top=.2:width=.3:height=.4:oi=1` | `data-shape="rect" data-left=".1" data-top=".2" data-width=".3" data-height=".4" data-occludeInactive="1" ` |
| `ellipse:left=.5:top=.5:rx=.1:ry=.2:oi=1` | `data-shape="ellipse" data-left=".5" data-top=".5" data-rx=".1" data-ry=".2" data-occludeInactive="1" ` |
| `polygon:left=.6:top=.1:points=.6,.1 .9,.1 .75,.3:oi=1` | `data-shape="polygon" data-left=".6" data-top=".1" data-points=".6,.1 .9,.1 .75,.3" data-occludeInactive="1" ` |
| `text:left=.1:top=.8:text=Label:scale=1.:fs=.05:oi=1` | `data-shape="text" data-left=".1" data-top=".8" data-text="Label" data-scale="1." data-font-size=".05" data-occludeInactive="1" ` |
| `rect:left=:top=0:width=0:height=.5:angle=0:fill=:oi=` | `data-shape="rect" data-top="0" data-height=".5" data-angle="0" ` |
| `ellipse:left=.1:top=.1:width=.2:height=0.0:rx=.1:ry=0:angle=2500` | `data-shape="ellipse" data-left=".1" data-top=".1" data-width=".2" data-height="0.0" data-rx=".1" data-angle="2500" ` |
| `text:left=.1:top=.1:text=:scale=0:fs=` | `data-shape="text" data-left=".1" data-top=".1" ` |
| `rect:left=.1:left=.2:zz=9:top=.3:points=1,2:text=t` | `data-shape="rect" data-left=".1" data-left=".2" data-top=".3" data-points="1,2" data-text="t" ` |
| `star:left=.1:top=.2` | `data-left=".1" data-top=".2" ` |
| `rect`, or nothing after the prefix | (empty) |
| `polygon:left=0:top=0:points=.1,.2  .3 .4,.5,.6 ,.7 .8,` | `data-shape="polygon" data-left="0" data-top="0" data-points=".1,.2 .4,.5,.6 ,.7 .8," ` |
| `polygon:left=0:top=0:points=abc` | `data-shape="polygon" data-left="0" data-top="0" ` |
| `rect:left=<b>:top="x":fill=a&b 'c'` | `data-shape="rect" data-left="<b>" data-top=""x"" data-fill="a&b 'c'" ` |
| `text:left=.1:top=.1:text=x:scale=00:fs=0` | `data-shape="text" data-left=".1" data-top=".1" data-text="x" data-scale="00" data-font-size="0" ` |
| `text:left=.1:top=.1:text=a\:b<c>"d"&e 'f' 日本\\x` LF `line 2:scale=1.5:fs=.0667` | `data-shape="text" data-left=".1" data-top=".1" data-text="a&#x3A;b&lt;c&gt;&quot;d&quot;&amp;e&#x20;&#x27;f&#x27;&#x20;日本&#x5C;x&#x0A;line&#x20;2" data-scale="1.5" data-font-size=".0667" ` (the value read is `a:b<c>"d"&e 'f' 日本\x` LF `line 2`) |

## I4. Cards and sides

The element is part of the `cloze:` output, so `render/cloze#C6` decides the rest:

- When no cloze of the field is active on the card (C6 step 1), the output is empty: no element at all, inactive and number-0 shapes included. A field whose masks are c1 to c3 shows nothing on card 4.
- Number 0 alone is never active, so a field holding only `{{c0::…}}` or `{{c0,2::…}}` shapes shows nothing on card 1.
- Text between the clozes (the `<br>` separators of `render/image-occlusion-format#F3`) is written as it is.
- The MathJax pass of `render/cloze#C8` runs over the whole output, elements included. A `fill` value holding `\(` in one shape and `\)` in a later one makes a MathJax match across the elements, and the markup inside the match is removed: the first element loses its `</div>` and the `<br>` after it (`render/image-occlusion/render/divs` records the result).
- A cloze inside an image-occlusion cloze is not written, and does not count for C6 step 1.

The usual case, a field with three masks c1, c2, c3 and a label c0, on card 2:

| Side | Classes, in field order |
|---|---|
| front | `cloze-inactive`, `cloze`, `cloze-inactive`, `cloze` (the label) |
| back | `cloze-inactive`, `cloze-highlight`, `cloze-inactive`, `cloze` (the label) |

Masks that share a number (a group) are all active together. A mask listed under several numbers (`{{c1,3::…}}`) is active on each of those cards and writes `data-ordinal="1,3"`.

## I5. The rest of an image-occlusion card

Nothing else about these cards is special in the core: the note type's templates place `{{cloze:Occlusion}}` and `{{Image}}` like any other fields, the image's name is encoded for display (`render/media-display#D1`), and type-in, AV extraction and FrontSide work as for any card. Which note types are image-occlusion note types matters only to the mask editor (`render/image-occlusion-format#F2`); the `cloze:` filter writes these elements for any note type whose field holds such clozes.

## I6. Hidden per-mask audio (R1020, R545; AE16)

Occlusion note types such as Tobira's keep one audio field per mask (`A01`, `A02`, …) holding `{{cN::[sound:…]}}`, shown with `{{cloze:A0N}}` inside an element the template hides. The core gives that behavior from the general rules, and Meguri adds nothing for it:

- On the front, the active cloze's content is written only inside `data-cloze`, attribute-encoded (`render/cloze#C6`, `#C7`): `[`, `:`, `.` and `]` become `&#x5B;`, `&#x3A;`, `&#x2E;`, `&#x5D;`, so the front has no sound tag and no audio (U62's extraction finds none).
- On the back, the active cloze's content is written as it is, inside its span, so its sound tag becomes the answer's play marker `[anki:play:a:0]` (and further numbers for further tags), inside the hidden element: the clip autoplays, and its replay button is in the hidden element, so none is visible.
- The audio fields of the other masks hold no cloze active on this card, so `cloze:` writes nothing for them (C6 step 1) and their audio is neither shown nor played.
- An inactive cloze's content is written on both sides (C6): a field that holds several masks' audio (`{{c1::[sound:a.mp3]}} {{c2::[sound:b.mp3]}}`) plays the inactive masks' audio too, on the front and on the back, exactly as in Anki.

AE16's card, mask 3 of three, field A03 `{{c3::[sound:x.mp3]}}`, template `…<div style="display:none">{{cloze:A01}}{{cloze:A02}}{{cloze:A03}}</div>`: the question ends `<div style="display:none"><span class="cloze" data-cloze="&#x5B;sound&#x3A;x&#x2E;mp3&#x5D;" data-ordinal="3">[...]</span></div>`, with no play marker; the answer ends `<div style="display:none"><span class="cloze" data-ordinal="3">[anki:play:a:0]</span></div>`, and its AV tags are exactly `x.mp3` (`render/image-occlusion/render/audio-tags`).

## I7. Scenarios

| Scenario | Covers |
|---|---|
| `render/image-occlusion/render/divs` | I1 to I4 through the `cloze` observation (`specs/conformance/cloze-ops.md`): classes by card and side, Hide All and Hide One fields, groups, each attribute rule, keys of other kinds, repeated and unknown keys, unknown names, no shape, points, raw values and the encoded label, number lists, number 0, nesting, a card with no shape, the MathJax pass |
| `render/image-occlusion/render/cards` | I4, I5 on a whole card through `review_html` (`specs/conformance/type-answer-ops.md` K2): three masks and a label on each card, the image name encoded |
| `render/image-occlusion/render/audio` | I6 through `review_html`: AE16's card and another mask's card |
| `render/image-occlusion/render/audio-tags` | I6 through U62's `render` observation: AE16's AV tag lists |

`render/image-occlusion/render/audio-tags` awaits its golden: the oracle driver gets the `render` kind with U62 (`specs/conformance/render-ops.md` Q5). Once that is on the integration branch, the golden is recorded with `tools/mg oracle record render/image-occlusion/render/audio-tags`; until then `goldens-check` reports it `missing`.

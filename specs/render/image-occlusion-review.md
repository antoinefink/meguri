# Image occlusion review runtime

Spec: render/image-occlusion-review
Requirements: R512 (`anki.imageOcclusion`, `anki.setupImageCloze`), R539, R540, R541, R542, R543, R544, R546, R1960; supports R1808, R1912, R1913
Anki: 26.09.3
Scenarios: conformance/scenarios/render/image-occlusion/review/ holds the shape markup the test fixtures draw from (M15). The drawing itself runs in Anki's web reviewer, which the oracle does not contain, so M14 is the verification.

What the image occlusion (IO) part of the card page does: the API that templates call (`anki.imageOcclusion`), how it reads the shape `div`s that the core renders into a side (U64, `render/image-occlusion-render`), how it sizes the image container and the canvas, how it draws masks and text labels, the Toggle Masks button and key, redraws, hooks, and what happens when something is missing. Anki desktop's reviewer is the reference; Anki ships the same runtime to its other clients as a separate bundle. Meguri's deliberate differences are in M13.

Not covered here: the page, its globals and its stylesheet (`render/card-runtime`, including the `#image-occlusion-container` and `#image-occlusion-canvas` rules of its C4.4), the shape markup (U64), the Occlusion field format (`render/image-occlusion-format`), per-mask audio (U64, U67), tap zones (U175) and the mask editor (U205).

## M1. Terms

- **Page**, **side**, **page world**, **presented**, **settled**: as in `render/card-runtime` C1 and C6.6. A page lives across many sides and cards.
- **Container**: the first element in the document whose id is `image-occlusion-container`. **Canvas**: the first element whose id is `image-occlusion-canvas`. **Image**: the first `img` element, in document order, that is a descendant of an element whose id is `image-occlusion-container`. Each is looked up again every time this spec says "the container", "the canvas" or "the image", so a later side's elements are found.
- **Toggle button**: the first element whose id is `toggle`.
- **Shape div**: an element the runtime reads as a shape (M3). **Mask**: a shape of kind `rect`, `ellipse` or `polygon`. **Label**: a shape of kind `text`.
- **Fractions**: shape positions and sizes in the markup are fractions of the image (`render/image-occlusion-format` F1). Horizontal values scale with the canvas width, vertical values with its height.
- **Backing size**: the canvas element's `width` and `height` (its bitmap, in device pixels). **Client size**: its `clientWidth` and `clientHeight` (CSS pixels, integers as the engine reports them). **DPR**: `window.devicePixelRatio`.
- **Draw pass**: one reading of the shapes, the hooks around it, and the drawing (M7).
- **Hidden state**: per page, either *all* (every shape is drawn) or *labels only* (M10).
- **JavaScript conversions** used below are ECMAScript's: `Number(t)` (whole-string numeric conversion: `""` and blanks give 0, `0x1A` gives 26, trailing junk gives NaN), `parseFloat(t)` (longest decimal prefix: `""` gives NaN, `5px` gives 5), `parseInt(t)` (decimal, leading digits), `Math.round` (halves toward +∞: 112.5 → 113, −0.5 → −0), `Math.floor`, `%` (remainder with the sign of the dividend) and number-to-string (`String(1.5)` is `1.5`, `String(4/3)` is `1.3333333333333333`). Arithmetic is IEEE double precision, in the order written.

## M2. The API (R512)

The runtime adds two members to the page's `anki` object (`render/card-runtime` C10.6), before any card script runs, as plain data properties (writable, enumerable, configurable):

- `anki.imageOcclusion`: a plain object whose own enumerable members are exactly, in this order: `setup` (M5), `drawShape` (M8), `Ellipse`, `Polygon`, `Rectangle`, `Shape`, `Text` (the classes of M4).
- `anki.setupImageCloze`: the same function object as `anki.imageOcclusion.setup`, so `anki.setupImageCloze === anki.imageOcclusion.setup` is true (a deprecated name that older templates call).

No page global is added (`render/card-runtime` C9). Both members exist on every platform (Mac, iPhone, iPad) and in every card page (review and previews). A card script that replaces either member or the object affects later calls in that page, as it would in Anki.

## M3. Reading shapes from the page

### M3.1 Which elements

A reading takes one of the three class names `cloze` (active), `cloze-inactive` (inactive) or `cloze-highlight` (highlight). It looks at every element in the whole document that has that class, in document order, and keeps those that:

- are HTML `div` elements, and
- have a `data-shape` attribute whose value is exactly `rect`, `ellipse`, `polygon` or `text` (case-sensitive).

Every other element is skipped without error: the `span`s of ordinary clozes (other fields of the note, or hidden per-mask audio fields, case IO10 of M15), divs without `data-shape` (unknown kinds such as `star` are rendered without it, case IO9), and shape-like elements of any other tag. Where the divs sit does not matter: the stock layout hides them in a `display: none` wrapper. An element with two of the classes is read in each reading it matches.

### M3.2 Attributes

Attribute names are those of the markup after HTML parsing, so the core's `data-occludeInactive` is read as `data-occludeinactive`. Values are the parsed attribute text, so `data-text="a&#x3A;b&#x0A;c"` is `a:b`, line feed, `c`.

| Shape property | Attribute | Value |
|---|---|---|
| `left`, `top` | `data-left`, `data-top` | `parseFloat(t)` when `Number(t)` is not NaN; 0 when it is NaN or the attribute is absent |
| `angle` (degrees) | `data-angle` | `((Number(t) % 10000) / 10000) × 360`; 0 when that is NaN (absent, non-numeric) |
| `fill` | `data-fill` | the text, when the attribute is present (even empty); otherwise the default: `#ffeba2` for masks, `#000000` for labels |
| `occludeInactive` | `data-occludeinactive` | `true` exactly when the text is `1`; `false` otherwise (absent, `true`, ` 1`) |
| `ordinal` | `data-ordinal` | `parseInt(t)`: `1,2` gives 1, absent gives NaN |
| `width`, `height` (rect) | `data-width`, `data-height` | `parseFloat(t)`; NaN when absent |
| `rx`, `ry` (ellipse) | `data-rx`, `data-ry` | `parseFloat(t)`; NaN when absent |
| `points` (polygon) | `data-points` | see below |
| `text` (label) | `data-text` | the text; `""` when absent |
| `scaleX`, `scaleY` (label) | `data-scale` | both `parseFloat(t)`; NaN when absent |
| `fontSize` (label) | `data-font-size` | `parseFloat(t)` when the text is not empty; `undefined` when the attribute is absent or empty |

Examples for `data-left`:

| Text | `left` |
|---|---|
| `.25`, `0.25`, `+.25`, `2.5e-1` | 0.25 |
| `1.` | 1 |
| absent, `abc`, `.5px` | 0 |
| `""`, `"  "` | NaN (`Number` gives 0, `parseFloat` gives NaN) |
| `0x10` | 0 (`Number` gives 16, `parseFloat` gives 0) |

Examples for `data-angle`: `2500` → 90, `12500` → 90, `-2500` → −90, `833` → 29.988, `10000` → 0, absent or `x` → 0, `""` → 0.

**Points.** When `data-points` is absent, the reading fails with a `TypeError` (M9). When it is present and empty, the polygon has one point, (0, 0) as numbers. Otherwise the text is split at every space (U+0020), and each item at its commas: the point's `x` is the text before the first comma and its `y` the text between the first and second comma; text after a second comma is dropped. An item without a comma has `y` the number 0, so two spaces in a row give the point (`""`, 0). Each point is an object with own properties `x` and `y`. `x` and `y` stay strings; they become numbers only when the shape is made absolute (M4.3), with `Number` semantics (`.5` → 0.5, `.5px` → NaN, `""` → 0).

### M3.3 Result

Each kept element gives one instance of the M4 class of its kind (`Rectangle`, `Ellipse`, `Polygon`, `Text`), built from the properties above, still in fractions. Properties not in the table get their M4 defaults. The `id` of each is new (M4.1). A reading returns a new array of these instances, in document order.

## M4. Shape classes

### M4.1 Construction and properties

`new Shape(options)`, `new Rectangle(options)`, `new Ellipse(options)`, `new Polygon(options)`, `new Text(options)`. `options` is an object (may be omitted); keys not listed are ignored. `Rectangle`, `Ellipse`, `Polygon` and `Text` are subclasses of `Shape`, so their instances are `instanceof anki.imageOcclusion.Shape`.

| Class | Options and defaults (a default applies when the key is absent or `undefined`) |
|---|---|
| all | `left` 0, `top` 0, `angle` 0, `fill` `#ffeba2`, `occludeInactive` (no default: `undefined`), `ordinal` (`undefined`) |
| `Rectangle` | also `width` 0, `height` 0 |
| `Ellipse` | also `rx` 0, `ry` 0 |
| `Polygon` | also `points` `[]` (an array of objects with `x` and `y`) |
| `Text` | also `text` `""`, `scaleX` 1, `scaleY` 1, `fontSize` (`undefined`); `fill` defaults to `#000000` instead |

An instance has these own enumerable properties, in this order: `left`, `top`, `angle`, `fill`, `occludeInactive`, `ordinal`, `id`, then `width`, `height` (rect), `rx`, `ry` (ellipse), `points` (polygon), or `text`, `scaleX`, `scaleY`, `fontSize` (label). `id` is `undefined` on a plain `Shape`, and on the others the kind (`rect`, `ellipse`, `polygon`, `text`), `-` and the current time in milliseconds since the epoch (`rect-1894701600000`).

### M4.2 Methods

`size` is any object with numeric `width` and `height`. "round" is `Math.round`.

| Method | Result |
|---|---|
| `absolutePosition(size)` | `{left: round(left × width), top: round(top × height)}` |
| `normalPosition(size)` | `{left: left / width, top: top / height}` |
| `toAbsolute(size)` | a new instance of the same class (a plain `Shape` gives a plain `Shape`) with every option property copied, a new `id`, and the M4.3 conversion |
| `toNormal(size)` | a new instance of the same class, the inverse: divisions, no rounding (M4.3) |

`toDataForCloze` and `toFabric` are not provided (M13 D4).

### M4.3 Absolute and normal values

| Kind | `toAbsolute(size)` | `toNormal(size)` |
|---|---|---|
| all | `left`, `top` as `absolutePosition` | as `normalPosition` |
| `Rectangle` | `width` = round(width × size.width), `height` = round(height × size.height) | `width / size.width`, `height / size.height` |
| `Ellipse` | `rx` = round(rx × size.width), `ry` = round(ry × size.height) | `rx / size.width`, `ry / size.height` |
| `Polygon` | `points`: a new array of plain objects `{x: round(x × size.width), y: round(y × size.height)}` | plain objects `{x: x / size.width, y: y / size.height}` |
| `Text` | `fontSize` = fontSize × size.height, not rounded, when fontSize is truthy (not 0, NaN or `undefined`); otherwise 40 | `fontSize` = (fontSize when truthy, otherwise 40) / size.height |

`scaleX`, `scaleY`, `angle`, `fill`, `occludeInactive`, `ordinal` and `text` are copied unchanged. NaN stays NaN. Example: a `Rectangle` with left 0.25, top 0.5, width 0.125, height 0.25, made absolute for 200 × 100, has left 50, top 50, width 25, height 25. For 1200 × 900, a height of 0.125 gives 113 (112.5 rounded up).

## M5. `setup(options)` (R539, R543)

### M5.1 The call

- `setup` never throws. It returns a promise that always fulfils with `undefined`.
- At the call it looks up the image. When there is one and it is not `complete`, it waits for that element's next `load` or `error` event. When that wait is over (otherwise in a microtask right after the call, never during it), the **setup step** (M5.2) is scheduled for the next animation frame and the promise fulfils. So the drawing has not happened yet when reactions to the promise run, nor when the template's script ends.
- `options` may be any value. The runtime never copies it: at each draw pass it reads `options.onWillDrawShapes` and `options.onDidDrawShapes` from it, treating a `null` or `undefined` `options` as having neither. A template that changes these members after the call changes later passes.
- Each call is independent: calling `setup` twice on one side runs two setup steps, two draw passes, and wires the toggle button twice (M10.1), as in Anki.

### M5.2 The setup step

At that animation frame, in this order:

1. When there is no canvas, the step ends here: nothing is drawn, nothing is changed, no listener is added, no error.
2. When there is no image, the container's content is replaced by the text `No image to show.` (as by assigning `innerText`, so the canvas and every other child of the container are gone), and the step ends. When there is no container either, this throws a `TypeError` (M9).
3. The container's inline `aspect-ratio` is set to the number-to-string of the image's `naturalWidth / naturalHeight` (`1.5` for 3000 × 2000, `1.3333333333333333` for 400 × 300). When that text is not a valid ratio (NaN, `Infinity`), the style ignores it and keeps any earlier inline value.
4. The backing size (M6) is assigned to the canvas's `width` and `height`, always, even when unchanged. Assigning clears the bitmap and resets the 2D context's state.
5. The first setup step in the page that gets this far registers the page's key listener (M10.2). Later ones do not.
6. The toggle button is wired (M10.1).
7. The page's hidden state becomes *all* (M13 D2).
8. A draw pass runs (M7).
9. Redraw watching (M11) moves to this side.

## M6. Backing size (R540)

From the image's natural size (nw, nh), the canvas's client size (cw, ch) and the DPR d, read at that moment (after M5.2 step 3, so the client size reflects the new aspect ratio):

1. s = min(cw × d / nw, ch × d / nh)
2. w = nw × s, h = nh × s
3. When w × h > 16 777 216 (4096 × 4096): k = 4096 / √(w × h); w = w × k; h = h × k
4. backing width = floor(w), backing height = floor(h)

So the backing covers the canvas's area in device pixels, at the image's aspect ratio, and the cap limits the **area** to 4096 × 4096 pixels, scaling both sides by the same factor. One side may exceed 4096 when the area stays under the cap.

| Image | Client size | DPR | Backing |
|---|---|---|---|
| 400 × 300 | 400 × 300 | 1, 2, 3 | 400 × 300, 800 × 600, 1200 × 900 |
| 3000 × 2000 | 1800 × 1200 | 1, 2 | 1800 × 1200, 3600 × 2400 |
| 3000 × 2000 | 1800 × 1200 | 3 | 5016 × 3344 (area capped: w = 5016.554…, h = 3344.369…) |
| 1000 × 1000 | 1400 × 1400 | 3 | 4096 × 4096 |
| 8000 × 1000 | 1600 × 200 | 3 | 4800 × 600 (not capped) |
| 1600 × 1200 | 350 × 262 | 2 | 698 × 524 (the client height limits s) |
| 1600 × 1200 | 350 × 262 | 3 | 1048 × 786 |

When the natural size is 0 × 0 (an image that failed to load), the results are NaN and the backing becomes 0 × 0 (the canvas's numeric attribute conversion), so nothing is visible.

## M7. Draw pass (R539)

### M7.1 Shapes

The pass reads (M3) the active, inactive and highlight shapes, in that order. In the hidden state *labels only*, each reading keeps only `Text` instances, before the hooks see them. When a reading throws (M3.2 points), the pass ends there: nothing is drawn and no hook is called.

### M7.2 Colours and borders

Read once per pass from the computed style of the canvas, as the text of each custom property (an empty text means the property is not set):

| Property in the hook data | Custom property | When not set |
|---|---|---|
| `activeShapeColor` | `--active-shape-color` | `#ff8e8e` |
| `inActiveShapeColor` | `--inactive-shape-color` | `#ffeba2` |
| `highlightShapeColor` | `--highlight-shape-color` | `#ff8e8e00` (transparent) |
| `activeBorder` | `--active-shape-border` | width 1, colour `#212121` |
| `inActiveBorder` | `--inactive-shape-border` | width 1, colour `#212121` |
| `highlightShapeBorder` | `--highlight-shape-border` | width 1, colour `#ff8e8e` |

A border text is split at spaces, empty pieces dropped. Its width is `parseFloat` of the first piece, or 1 when that is NaN (also when there is no piece). Its colour is the second piece as written, or the default colour when there is none. So `2px #00ff00` gives 2 and `#00ff00`; `2px` gives 2 and the default colour; `#212121 1px` gives 1 and `1px`; `1px solid red` gives 1 and `solid`. Colour texts are never checked or normalized: they go to the canvas as they are, and the canvas ignores an invalid one, keeping its current colour (M7.5).

The properties come from the canvas's own computed style, so a template sets them on the canvas or an ancestor. Night mode changes nothing by itself (R492): only rules the template writes for `.nightMode` do.

### M7.3 `onWillDrawShapes`

When `options.onWillDrawShapes` is not `null` or `undefined`, it is called with two arguments and no receiver:

1. **data**: a new object `{activeShapes, inactiveShapes, highlightShapes, properties}`: the three arrays of M7.1 and `properties`, a new object `{activeShapeColor, inActiveShapeColor, highlightShapeColor, activeBorder: {width, color}, inActiveBorder: {width, color}, highlightShapeBorder: {width, color}}` from M7.2. The shapes are in fractions; `inactiveShapes` holds every inactive shape, whatever its `occludeInactive`.
2. **context**: the canvas's 2D context.

When it returns a truthy value R, the pass goes on with `R.activeShapes`, `R.inactiveShapes`, `R.highlightShapes` and `R.properties`, read from R whatever they are. Otherwise it goes on with the data object's members, including changes the hook made to them in place. A value that is not callable and not `null` or `undefined` makes the pass throw a `TypeError` here. A throw ends the pass: nothing is drawn.

### M7.4 Drawing

Shapes are drawn with `drawShape` (M8) on the canvas's 2D context with `size` equal to the canvas's backing size, in this order:

| Shapes | `fill` | `stroke`, `strokeWidth` |
|---|---|---|
| every active shape, in array order | `activeShapeColor` (the shape's own fill is ignored) | `activeBorder` colour and width |
| the inactive shapes whose `occludeInactive` is truthy, in array order | the shape's own `fill`, except when it is exactly the text `#ffeba2`: then `inActiveShapeColor` | `inActiveBorder` |
| every highlight shape, in array order | `highlightShapeColor` | `highlightShapeBorder` |

So in Hide One mode (no `oi`) the other masks are not drawn and the image shows through, and on the answer side the card's own mask is, with the stock colours, a transparent fill with a 1-pixel outline. The `#ffeba2` comparison is exact text: a mask whose `data-fill` is `#FFEBA2` is drawn in that colour, not in `inActiveShapeColor`. Labels are drawn wherever they are listed, with their own colours (M8.4); in practice they are number 0, so `cloze` on both sides (`render/cloze` C11), and appear on every card and side.

### M7.5 `onDidDrawShapes` and context state

- After drawing, when `options.onDidDrawShapes` is not `null` or `undefined`, it is called with a new object `{activeShapes, inactiveShapes, highlightShapes, properties}` holding the values the pass drew with, and the context. Its return value is ignored. A throw ends the pass after the drawing.
- The pass itself never clears the canvas: the setup step and redraws clear it by assigning the backing size (M5.2 step 4, M11), the toggle with an explicit clear (M10.3).
- `drawShape` assigns the fill colour, stroke colour and line width to the context for every shape, even when unchanged, and leaves them assigned. Everything else it sets is restored, except the context's current path, which stays the last ellipse or polygon path. So a colour the canvas rejects leaves the previous shape's colour in effect (the context default, black, right after a size assignment), and context changes a hook makes stay in effect for the rest of the pass, and for later toggles, until the next size assignment.

## M8. `drawShape(parameters)` (R540)

`parameters` is one object `{context, size, shape, fill, stroke, strokeWidth}`; calling with no argument throws a `TypeError`. It returns `undefined`.

1. The shape is made absolute with `size` (M4.2). A value without `toAbsolute` throws a `TypeError`.
2. `fill`, `stroke` and `strokeWidth` are assigned to the context's fill style, stroke style and line width, as given. The canvas ignores values it rejects: an invalid colour, or a line width that is 0, negative, infinite or NaN after numeric conversion (`undefined` converts to NaN).
3. The absolute shape is drawn according to its class, below. An instance of none of the four subclasses (a plain `Shape`) draws nothing.

θ is the shape's `angle` in degrees × π / 180 (`angle` `undefined` or `null` counts as 0). When θ is neither 0 nor NaN, a rect, ellipse or label is rotated by θ about its absolute top-left point (`left`, `top`): clockwise on screen for positive θ, since the canvas's y axis points down. The stroke is drawn only when `strokeWidth` is truthy (not 0, NaN or `undefined`), because a canvas strokes even at width 0.

### M8.1 Rect

Fill the rectangle (`left`, `top`, `width`, `height`), then stroke the same rectangle.

### M8.2 Ellipse

One closed path: the full ellipse centred at (`left` + `rx`, `top` + `ry`) with radii `rx` and `ry`, axis-aligned before the rotation. Fill it, then stroke it. A NaN radius (attribute absent) gives an empty path, so nothing is drawn. A negative radius makes the canvas throw an `IndexSizeError`, which ends the pass (M9).

### M8.3 Polygon

- The offset is (`left` − mx, `top` − my), where mx and my are the smallest absolute x and y of the points: start from the first point's coordinate and take any later point's coordinate that is smaller (so a NaN in the first point makes the minimum NaN, and NaN in later points is passed over). An empty `points` array throws a `TypeError`.
- One closed path through the absolute points in order, each moved by the offset, from the first point back to it. Fill it, then stroke it.
- `angle` is ignored: polygons are never rotated.

So a polygon whose `left` and `top` equal its smallest point coordinates is drawn exactly at its points, and one with other values is moved to start there (case IO6 of M15).

### M8.4 Label

The `fill`, `stroke` and `strokeWidth` arguments do not affect a label. With the label's absolute values (M4.3), and its `scaleX` and `scaleY` written sx and sy:

- **Font**: `F px Arial`, where F is `fontSize` written with number-to-string (`72px Arial`), with the text baseline at the top of the em box. F is `data-font-size` × backing height, or 40 when the label has none (M4.3).
- **Scale**: the label is drawn in a coordinate system scaled by (sx, sy). Its origin in those units is (x0, y0) = (`left` / sx, `top` / sy), which is the point (`left`, `top`) on the canvas.
- **Lines**: the text is split at line feeds (U+000A). Line i (from 0) starts at (x0, y0 + i × L), where the line height L is 1.5 × the ink height of the letter `M` in that font: the sum of the actual bounding box ascent and descent that the canvas measures for `M`.
- **Box**: first a rectangle at (x0, y0) of width W + 5 and height n × L + 5 is filled with `#ffffff`, where W is the widest line's measured width (0 when every line is empty) and n the number of lines. The 5 units of padding are on the right and bottom only, and scale with the label.
- **Text**: then each line is filled at its position in the label's own `fill` (`data-fill` as written, or `#000000`; `#000` when a label built by a template has `fill` `null` or `undefined`).
- **Rotation**: when θ is neither 0 nor NaN, box and text are rotated together about (x0, y0) in the scaled units, which is the label's top-left on the canvas. Measuring happens before and is not affected.
- Non-finite values make the canvas ignore the affected calls: a label without `data-scale` (sx NaN), or whose scale is 0 (`.0000`), draws nothing.

## M9. Failures and the template's `#err` (R544)

`setup` never throws and its promise always fulfils (M5.1). A failure after the call is reported through the page's own error events, which `render/card-runtime` C12 records for the "Template Errors…" view:

- in a setup step or a draw pass it starts: an unhandled promise rejection (C12 kind `rejection`);
- in a toggle by click or key (M10): an uncaught exception in that event listener (C12 kind `exception`, phase `later`);
- in a redraw (M11): either of the two.

| Situation | What the page shows | Reported |
|---|---|---|
| no canvas | nothing changes | nothing |
| canvas, no image | the container holds only the text `No image to show.` | nothing |
| canvas, no image and no container | nothing changes | `TypeError` |
| the image fails to load (404, undecodable) | no masks; no aspect ratio is set, so the container, with the base stylesheet, has no height of its own | nothing |
| a polygon div without `data-points` in any of the three readings | no masks at all in that pass (case IO8) | `TypeError` |
| `onWillDrawShapes` neither callable, `null` nor `undefined` | no masks | `TypeError` |
| `onDidDrawShapes` neither callable, `null` nor `undefined` | the masks | `TypeError` |
| `onWillDrawShapes` throws | no masks | its exception |
| `onDidDrawShapes` throws | the masks | its exception |
| an ellipse with a negative radius | the shapes drawn before it | `IndexSizeError` |
| a toggle (click or key) while there is no canvas | nothing changes | `TypeError` |

**`#err`.** The runtime never writes `#err` or any element outside the container. A template that calls `setup` inside `try`/`catch` and writes `#err` in its handler writes it only when its own code throws synchronously, for example when `anki.imageOcclusion` is missing, as on a client without this runtime. With Meguri's runtime in place, `#err` therefore stays as the template left it in every row of the table, exactly as in Anki. This is what "the template's own `#err` text, as in Anki" (R544) amounts to (M17 item 2).

**UI string.** `No image to show.` is Anki's English wording and Meguri uses it exactly (R195: English only).

## M10. Toggle Masks (R541)

### M10.1 The button

At step 6 of each setup step (M5.2), when there is a toggle button:

- when some element anywhere in the document has a `data-occludeinactive` attribute whose value is exactly `1` (a shape in Hide All mode), a `click` listener is added to the button. Clicking it toggles (M10.3) with the `options` of that setup call;
- otherwise the button's inline `display` is set to `none`.

Nothing else about the button changes: when visible, its look comes from the template. The stock templates have the button on the answer side only, so in practice it shows on the back of Hide All notes. A template with a button on the front gets the same behavior there.

### M10.2 The key

The page's key listener is a `keydown` listener on `window`, added once per page (M5.2 step 5) and never removed. For each `keydown` whose `key` is exactly `M` (Shift+M, or M with Caps Lock on; modifier keys are not checked), when there is a toggle button and its inline `display` is not `none`, it toggles (M10.3) with the `options` of the latest setup call in the page (M13 D3). It never calls `preventDefault` or `stopPropagation`, so Meguri's own key handling still sees the key afterwards (R426). A button hidden by the template's CSS rather than its inline style still counts as visible.

### M10.3 Toggling

1. The page's hidden state flips: *all* becomes *labels only* and back.
2. The canvas's whole bitmap is cleared: the rectangle from (0, 0) of the backing size, under the context's current transform.
3. A draw pass runs (M7) with the new hidden state and the toggling `options`.

Toggling does not recompute the aspect ratio or backing size, adds no listener and does not re-check `oi`. So on the back of a Hide All note, the first toggle leaves only the labels (the image shows everywhere else) and the second draws every shape again.

## M11. Redraws on size changes (R542)

Anki desktop sizes the canvas once per side and lets the browser stretch it (M13 D1). Meguri redraws instead, so the masks stay sharp and labels keep the size rules of M8.4 after rotation, Split View, Stage Manager, folding, a Mac window resize and card zoom.

- **Triggers**, after a setup step got past its image check: a change of the container's size (a `ResizeObserver` on it), a `resize` event on `window`, and a change of the DPR (for example a Mac window moving to a display with another scale, or a zoom that changes it).
- **Redraw**: at the next animation frame after one or more triggers, when the container watched is still in the document and the canvas's client size or the DPR differs from the values used at the side's last backing-size assignment: the backing size is recomputed (M6) and assigned, which clears the canvas, then a draw pass runs (M7) with the current hidden state and the `options` of the latest setup call. Triggers within one frame give one redraw; triggers that change nothing give none. So the observer's first report and a hook that changes nothing about the size cause no extra pass.
- A redraw does not call `setup`, does not set the aspect ratio again, adds no listener, does not wire the button again and keeps the hidden state.
- There is one watch per page. Each setup step that gets past its image check moves it to its own container. When the side is replaced, the watched container leaves the document and size changes draw nothing until the next IO side.
- An appearance change (light or dark, R490) is not a trigger: as in Anki, the masks keep the colours of their last pass until the next pass.

## M12. Page integration (R543, R546, R1808)

- **One-time state.** Per page, kept in the page-world runtime's closure (no global): whether the key listener exists, the latest setup call's `options`, the hidden state and the redraw watch. It lives as long as the page, across sides and cards (R543). Pages share nothing.
- **Pre-rendered questions** (R445, R1808). In an unpresented page, the first draw pass of a side happens before the page reports `settled` (`render/card-runtime` C6.6), when the image has loaded within the settle wait (its 800 ms cap). So a presented question shows its masks in its first frame. When the image is late, the masks follow its load as in Anki. Until the setup step sets the aspect ratio, the container, with the base stylesheet, has no height of its own, so no unmasked image is visible.
- **Listener registry** (`render/card-runtime` C13). The key listener counts as a card `keydown` listener on `window`, at least while the document has a toggle button whose inline `display` is not `none` (it may count always). Each `click` listener on a toggle button counts as a card tap listener on that element. The runtime adds no other listener of the tap set or the key set: none on the canvas, the image or the container, and none on `window`, `document` or `<body>`. A tap on the image or the canvas is therefore an ordinary tap-zone tap (R546), and a tap on the button is a tap on interactive content (U175).
- **No styling** beyond M5.2 step 3 (inline `aspect-ratio` on the container), the `innerText` of M5.2 step 2 and the button's inline `display`. No `pointer-events`, `touch-action`, `cursor` or other style on the canvas, image or container.

## M13. Deliberate differences from Anki desktop

The oracle cannot observe the reviewer page (M15), so these do not go into `conformance/deviations.yaml`. Each has a test; the names are suggestions for `ImageOcclusionReviewTests`.

| Id | Requirement | Anki desktop | Meguri | Test |
|---|---|---|---|---|
| D1 | R542 | sizes the canvas once per side; on a resize the browser stretches it, so resolution and the 40-pixel size of labels without `fs` stay those of the first layout. Its resize handler is added only when the page's `load` event comes after the call, which never happens for a card in its long-lived page. In a page where it does, each resize repeats the whole setup and adds another button listener, so after one resize a click toggles twice | M11: a redraw at the new size without repeating the setup | `redrawOnResize`, `redrawOnZoom`, `redrawKeepsSingleToggle` |
| D2 | R541 | the hidden state belongs to the page and survives into later cards, so after hiding on one card the first toggle on a later card seems to do nothing | reset to *all* at each setup step (M5.2 step 7) | `hiddenStateResetsPerSide` |
| D3 | R541, R543 | the key uses the `options` of the first setup call in the page | the latest setup call's `options` (M10.2) | `keyUsesLatestOptions` |
| D4 | R512 | shapes also have `toDataForCloze()` (the Occlusion field serialization) and `toFabric(size)` (objects of the editor's canvas library, bundled into the reviewer) | not provided: editor helpers with no known template use; Meguri's editor is native (U205) and bundles no such library | `shapeMethodsSurface` |

Also not reproduced, with no visible effect: Anki's fallback that sizes the container in pixels when the engine lacks `aspect-ratio` (WebKit has it), and its page-`load` listener per call (C2.4 of `render/card-runtime`: card scripts run after `load`).

## M14. Verification cases

Synthetic content only (R1959, R1960): Meguri's own note types, CSS and generated images. A case's side HTML is passed to the card runtime as `render/card-runtime` C16 does, in the page of U67 or the WebTestHarness, on the macOS host and the iOS simulator. Expected values come from this spec, never from the runtime under test: pixel positions and backing sizes are computed by the test from M4.3 and M6, using the DPR the page reports.

### M14.1 Fixture page

- **Image**: `_io.png`, a generated opaque PNG, 400 × 300, solid `#808080`, in the test's media folder. SZ cases use other sizes, given there.
- **CSS** (the side's `<style>`):

  ```css
  #image-occlusion-canvas {
    --active-shape-color: #ff0000;
    --inactive-shape-color: #0000ff;
    --highlight-shape-color: #ffff0000;
    --active-shape-border: 0px #000000;
    --inactive-shape-border: 0px #000000;
    --highlight-shape-border: 1px #00ff00;
  }
  #image-occlusion-container { width: 400px; max-height: none; }
  ```

- **Question body**: `<div style="display:none">` + the golden question output of the case (M15) + `</div><div id="err"></div><div id="image-occlusion-container"><img src="_io.png"><canvas id="image-occlusion-canvas"></canvas></div><script>try { anki.imageOcclusion.setup(window.__ioOptions) } catch (e) { document.getElementById("err").textContent = "setup threw: " + e }</script>`.
- **Answer body**: the same with the golden answer output, followed by `<div><button id="toggle">Masks</button></div>`.
- `window.__ioOptions` is set by the test before the side (undefined unless the case says otherwise). `log(x)` appends to a page-world array the test reads.
- **Pixels** are read with `getImageData` on the canvas, in backing coordinates, at the centre of a shape unless stated. W × H is the backing size (400d × 300d at DPR d). Colours are exact: `#ff0000` is (255, 0, 0, 255); "transparent" means alpha 0, where the image shows through.
- **Drawing calls** may be recorded instead of pixels by wrapping `CanvasRenderingContext2D.prototype` methods and setters in the page before the side (a test-only script): useful for fonts, lines and transforms.

### M14.2 Masks and colours (R539)

| # | Fixture | Observed |
|---|---|---|
| MC1 | IO1 question (Hide All, card 2) | c2 centre (0.5W, 0.175H) red; c1 (0.15W, 0.175H) and c3 (0.8W, 0.175H) blue; label box (0.05W + 2, 0.6H + 2) `#ffffff`; (0.5W, 0.45H) transparent |
| MC2 | IO2 question (Hide One, card 2) | c2 red; c1 and c3 transparent; label box white |
| MC3 | IO1 answer | c2 centre transparent; a pixel on its left edge (round(0.4W), 0.175H) has alpha > 0 and green as its largest channel; c1 and c3 blue; label white |
| MC4 | IO2 answer | c2 centre transparent with a green edge; c1, c3 transparent; button inline `display` is `none` |
| MC5 | IO3 question (card 1) | c1 red (own fill ignored); c2 `#00ff00`; c3 blue (`#ffeba2` written out is the default); c4 (255, 235, 162) (`#FFEBA2` is not the default text); c5 ellipse centre (0.15W, 0.6H) blue; c6 polygon at (0.5W, 0.55H) `#ff00ff` |
| MC6 | IO10 question | c1 red; c2 blue; `onWillDrawShapes` sees 1 active and 1 inactive shape (the audio field's `span.cloze` is skipped) |
| MC7 | IO9 question | only the last rect is red; (0.2W, 0.2H), (0.5W, 0.2H) and (0.2W, 0.6H) transparent; `activeShapes` has 3 entries (the kind-less div is skipped; the zero-width rect and radius-less ellipse are read and draw nothing) |
| MC8 | IO7 question (card 2) | the `1,2` rect is red; its `ordinal` in the hook data is 1; the c3 rect blue |
| MC9 | IO1 question, no CSS custom properties | c2 `#ff8e8e` with a 1-pixel `#212121` border; c1 `#ffeba2` |

### M14.3 Geometry (R540)

| # | Fixture | Observed |
|---|---|---|
| GE1 | IO5 question | rect 1 (left round(0.25W), top round(0.25H), width round(0.25W), height h = round(0.125H)) lies rotated 90° clockwise about its top-left: the pixel at (left − h/2, top + width/2) is red; its unrotated centre (left + width/2, top + h/2) is transparent. At DPR 2: left 200, top 150, width 200, h 75, sample (162, 250) red, (300, 187) transparent |
| GE2 | IO5 question | the ellipse's centre is at (round(0.6W) − round(0.05H), round(0.2H) + round(0.1W)): red; its unrotated centre (round(0.6W) + round(0.1W), round(0.2H) + round(0.05H)) transparent |
| GE3 | IO5 question | rect 3 (`angle` 12500) is drawn exactly as with 2500; the c2 rect (inactive, no `oi`) is not drawn |
| GE4 | IO6 question | triangle 1 is drawn moved by (0.4W, 0.4H): (0.6W, 0.5667H) red, (0.2W, 0.1667H) transparent; the square of polygon 2 is not rotated: (0.2W, 0.7H) red, and its corners (0.1W + 2, 0.6H + 2) and (0.3W − 2, 0.8H − 2) red |
| GE5 | IO4 question, recorded calls | label `Label`: font size 0.1 × H px (90 at DPR 3), `fillText("Label", round(0.1W), round(0.1H))` with identity scale; label `Plain`: font `40px Arial`; label `a:b`/`c`: font 0.05 × H px, scale 2, two `fillText` calls `a:b` and `c` whose y differ by 1.5 × the ink height of `M`, rotation 90° about (round(0.5W)/2, round(0.1H)/2) in scaled units, the box filled first with `#ffffff`, the text in `#0000ff`; label `Unscaled`: no visible pixel at (0.5W + 4, 0.7H + 4) |
| GE6 | IO4 question | the white box of `Label` starts at (round(0.1W), round(0.1H)) and is W_text + 5 wide and 1.5 × ink(M) + 5 high in backing pixels, ±1 pixel, where W_text and ink(M) are the canvas's own `measureText` results in that font |

### M14.4 Sizes (R540)

| # | Setup | Observed |
|---|---|---|
| SZ1 | IO1 question, `_io.png` 400 × 300 | backing 400d × 300d; the container's inline `aspect-ratio` reads `1.3333333333333333` (or `1.3333333333333333 / 1`, as the engine serializes it), and its height is 300 px |
| SZ2 | image 3000 × 2000, container CSS `width: 1800px` | DPR 1, 2, 3: backing 1800 × 1200, 3600 × 2400, 5016 × 3344 |
| SZ3 | image 8000 × 1000, container `width: 1600px` | DPR 3: 4800 × 600 |
| SZ4 | image 1000 × 1000, container `width: 1400px` | DPR 3: 4096 × 4096 |
| SZ5 | IO1 question on iPhone (DPR 3) and Mac (DPR 2 or 1) | the M6 formula with the page's own client size and DPR |

### M14.5 Toggle (R541)

| # | Setup | Observed |
|---|---|---|
| TG1 | IO1 answer; click `#toggle` | c1, c3 and the c2 edge transparent; label still white; one more `onWillDrawShapes` call, whose arrays hold only the label |
| TG2 | then `keydown` with `key` `M` on `window` | everything as in MC3 again |
| TG3 | IO1 answer; `keydown` `m` | nothing changes, no pass |
| TG4 | IO2 answer: `keydown` `M`, and a click on the hidden button | nothing changes, no pass |
| TG5 | IO1 question (no button); `keydown` `M` | nothing changes |
| TG6 | IO1 answer; the template's CSS hides `#toggle` (`display: none` in a rule); `keydown` `M` | toggles (only the inline style counts) |
| TG7 | IO1 answer of card A, toggled to labels only; next card B in the same page, IO1 answer; one click | labels only (the first click on B hides; D2) |

### M14.6 Redraws (R542)

| # | Setup | Observed |
|---|---|---|
| RD1 | IO1 question; container CSS width `auto` instead of 400px; the web view resized from 390 to 700 pt wide | one more pass; the new backing equals M6 for the new client size; c2 still red at (0.5W, 0.175H) of the new W × H |
| RD2 | the same, page zoom (card zoom of U70) from 100% to 150% | one more pass at the M6 size for the new client size and DPR |
| RD3 | a `resize` event with no size change; a hook that changes nothing | no further pass |
| RD4 | IO1 answer toggled to labels only, then a resize | the redraw draws only the label |
| RD5 | after the IO side, a plain side in the same page; resize | no pass, no error |
| RD6 | IO1 answer, three resizes, then one click | the toggle hides once (no extra click listener, D1) |

### M14.7 Hooks and one-time state (R543)

| # | Setup | Observed |
|---|---|---|
| HK1 | IO1 question; `__ioOptions = {onWillDrawShapes: (d, c) => log(["will", d.activeShapes.length, d.inactiveShapes.length, d.highlightShapes.length, d.properties.activeShapeColor, d.properties.inActiveBorder.width, c === canvas.getContext("2d")]), onDidDrawShapes: () => log("did")}` | log `["will", 2, 2, 0, "#ff0000", 0, true]`, then `did`; `d.activeShapes[0] instanceof anki.imageOcclusion.Rectangle`, `d.activeShapes[1] instanceof anki.imageOcclusion.Text`; `d.inactiveShapes[0].left === 0.05` |
| HK2 | IO1 question; `onWillDrawShapes` returns `{activeShapes: [], inactiveShapes: d.inactiveShapes, highlightShapes: [], properties: Object.assign({}, d.properties, {inActiveShapeColor: "#00ffff"})}` | c2 and the label transparent; c1 and c3 `#00ffff`; `onDidDrawShapes` receives the returned arrays |
| HK3 | IO1 question; `onWillDrawShapes` sets `d.activeShapes.length = 0` and returns nothing | c2 and the label transparent; c1, c3 blue |
| HK4 | `anki.imageOcclusion.setup(__ioOptions).then(() => log(willCount))` | logs 0 |
| HK5 | two IO1 cards in one page (A then B, both answers shown); one `keydown` `M` on B | exactly one pass; the page has registered one IO `keydown` listener in total |
| HK6 | as HK5, A's and B's options log different names | the key's pass calls B's hooks (D3) |
| HK7 | IO1 answer; after the first pass the test replaces `__ioOptions.onWillDrawShapes`; click | the new function is called |
| HK8 | IO6 question; hook reads `d.activeShapes[0].points[0]` | `{x: ".1", y: ".1"}`: strings |

### M14.8 Failures (R544)

| # | Setup | Observed |
|---|---|---|
| ER1 | IO1 question without the `<img>` | the container's text is `No image to show.`; no canvas in the document; `#err` empty; no diagnostic |
| ER2 | IO1 question, `<img src="_missing.png">` | no masks, backing 0 × 0, no inline `aspect-ratio`; `#err` empty; no diagnostic from the runtime (the 404 is U67's) |
| ER3 | IO8 question | c2 transparent; no hook called; one `rejection` diagnostic whose message starts with `TypeError`; `#err` empty |
| ER4 | IO1 question, `onWillDrawShapes` throws `Error("x")` | nothing drawn; a `rejection` diagnostic `Error: x`; `#err` empty |
| ER5 | `anki.imageOcclusion.setup(5)`, `setup(null)`, `setup()` on a side without canvas | each returns a promise that fulfils; no throw; no change |
| ER6 | a template that writes `#err` when `typeof anki.imageOcclusion.setup !== "function"` | `#err` empty on every platform |

### M14.9 API and page (R512, R546, R1808)

| # | Check | Expected |
|---|---|---|
| AP1 | `anki.setupImageCloze === anki.imageOcclusion.setup`; `Object.keys(anki.imageOcclusion)` | `true`; `setup, drawShape, Ellipse, Polygon, Rectangle, Shape, Text` |
| AP2 | `new anki.imageOcclusion.Rectangle()`; `new anki.imageOcclusion.Text()` | keys and defaults of M4.1; `id` matches `rect-` + digits; `fill` `#000000` for the label |
| AP3 | `new Rectangle({left: .25, top: .5, width: .125, height: .25}).toAbsolute({width: 200, height: 100})` | left 50, top 50, width 25, height 25, a `Rectangle` |
| AP4 | `drawShape({context: c, size: {width: 100, height: 100}, shape: new Rectangle({left: .1, top: .1, width: .5, height: .5}), fill: "#ff0000", stroke: "#000000", strokeWidth: 0})` on a test canvas 100 × 100 | (35, 35) red, (80, 80) transparent; returns `undefined` |
| AP5 | `typeof new Rectangle().toDataForCloze`, `typeof new Rectangle().toFabric` | `"undefined"` twice (D4) |
| AP6 | the C13 listener registry on IO1's answer | tap on canvas, image or container: no; tap on `#toggle`: yes; `keydown` on `window`: yes |
| AP7 | unpresented page, IO1 question, image served 300 ms late | when `settled` arrives, c2 is already red |
| AP8 | the page-world globals diff of `render/card-runtime` C9.4 after IO sides | unchanged: no new global |

The corpus cases of R1912 (the user's IO and Tobira-style note types, anonymized, on iPhone, iPad and Mac, light and dark: no `#err` text, a non-transparent canvas) and the hand-captured screenshots of R1913 belong to the suites of U240 and later.

## M15. Oracle, scenarios and goldens

- Anki's IO review runtime ships in its web reviewer, which the oracle does not contain, so nothing in M5–M12 can be recorded. The M14 expectations come from this spec.
- `conformance/scenarios/render/image-occlusion/review/fixtures.yaml` records, through the `template` observation (`specs/conformance/render-ops.md`), the question and answer markup of the ten fixture notes IO1–IO10 on a synthetic cloze note type. Its golden is the side HTML that M14 places in its pages: the fixtures take it from the golden, never from Meguri's renderer.

  | Fixture | Content | Card |
  |---|---|---|
  | IO1 | three rects c1–c3 and a label c0, Hide All (`oi=1`) | 2 |
  | IO2 | the same without `oi` (Hide One) | 2 |
  | IO3 | own fills: c1 `#123456`, c2 `#00ff00`, c3 `#ffeba2`, c4 `#FFEBA2`, c5 ellipse, c6 polygon `#ff00ff`, Hide All | 1 |
  | IO4 | labels: with `fs`, without `fs`, rotated two-line `a:b`/`c` at scale 2 in `#0000ff`, without `scale`; a c1 rect | 1 |
  | IO5 | rotations: rect and ellipse at 2500 steps, rect at 12500; an inactive c2 rect | 1 |
  | IO6 | polygons: offset triangle, square with an ignored angle | 1 |
  | IO7 | one rect on c1 and c2 (`data-ordinal="1,2"`), one on c3, Hide All | 2 |
  | IO8 | a polygon without points (c1), a rect (c2) | 2 |
  | IO9 | an unknown kind, a zero-width rect, an ellipse without `ry`, a rect | 1 |
  | IO10 | two rects with hidden per-mask audio cloze fields beside them | 1 |

- The scenario is not active in `conformance/scenarios/render/area.yaml`: Meguri renders IO clozes only once U64 fills the `cloze:` hook (`render/cloze` C11). The unit that lands that hook marks it active.

## M16. Implementation constraints

The traps to add to `app/Packages/MeguriKit/Sources/CardRendering/Resources/CardRuntime/AGENTS.md`:

- **No Anki assets** (R1960). `image-occlusion.js` is written from this spec. The similarity scan covers it.
- **No page globals.** Only the two `anki` members of M2. The one-time state lives in the runtime's closure.
- **JavaScript semantics are the spec.** `Number` versus `parseFloat` (M3.2), `Math.round` halves up (M4.3), points kept as strings (M3.2), `%` with the sign of the dividend (M3.2), operation order in M6. Colour strings go to the canvas untouched, and fill, stroke and line width are assigned for every shape (M7.5).
- **Never throw from `setup`**, and never resolve its promise after drawing (M5.1).
- **No listeners or styles on the canvas, image or container** (M12).
- **Read hooks at each pass** from the options object (M5.1).

## M17. Open points

Defaults this spec takes until the owner decides otherwise:

1. **Backing cap.** The U68 test list says the backing is "natural size × 3 capped at 4096 per side". Anki sizes the backing from the canvas's client size and caps its **area** at 4096 × 4096, scaling both sides (M6, SZ3). This spec follows Anki (R540: "as Anki does"); the U68 test is SZ2–SZ4.
2. **Missing image and `#err`.** The U68 approach and test list say a missing image puts the template's error text into `#err`. Anki writes `No image to show.` into the container and never touches `#err`; the template's handler runs only when the call itself throws, which `setup` never does (M9). This spec follows Anki, as R544 asks ("as in Anki"); the U68 test is ER1 and ER6.
3. **Redraw triggers.** M11 adds a DPR change to R542's list and skips passes that would change nothing. Both are Meguri's choices under R542.
4. **D2 and D3.** Anki carries the hidden state and the first call's options across cards. Meguri's two alternating pages would make that depend on card parity, so this spec resets the state per setup step and uses the latest options.
5. **Settling** (M12). `render/card-runtime` C6.6 does not list IO drawing among what an unpresented page waits for. This spec adds it: the U68 implementer extends the runtime's settle wait (bounded by the same 800 ms) so the first draw pass comes before `settled`.
6. **Pending specs.** `render/card-runtime` (U66) and `render/image-occlusion-format` (U63) are on their spec branches, and `render/image-occlusion-render` (U64) is being written. This spec reads the markup the oracle emits (M15 golden); if U64 deviates from it, M3 still describes what the runtime reads.

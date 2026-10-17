# Card runtime: page skeleton, base CSS, night mode and card JavaScript surface

Spec: render/card-runtime
Requirements: R466–R474, R476, R478, R479–R489, R491, R492, R499 (preload and script waits), R504–R510, R512 (except image occlusion), R513–R522, R534–R536, R563, R565, R1960, R2038 (page side); AE15, AE17 (IME part), AE18 (page part)
Anki: 26.09.3
Scenarios: none. Anki's reviewer page lives in its desktop GUI package, which the oracle does not contain, so nothing here can be recorded as a golden (C17). The cases in C16 are what the runtime test suites assert.

What a card page is and does from the point of view of card templates: the document Meguri loads, the stylesheet that comes before the card's CSS, how a side is inserted and its scripts run, and the globals card scripts can use. Anki desktop's reviewer page is the reference. Where AnkiMobile differs and a requirement follows AnkiMobile, the section says so. Every deliberate difference is in C15.

Not covered here: serving media and the page document from the media origin and the website data store (U67, `specs/render/media-serving.md`), native audio (U67), the image occlusion runtime (U68), links, dialogs, zoom and page recovery (U70), the rendered side HTML itself (U62, U64) and tap and key decisions (U175, U180, U181).

## C1. Terms

- **Page**: one long-lived card document in one web view. A review session has two pages that take turns, card by card (R465). Both sides of a card are shown in the same page. Card previews use a page of their own. Everything in this spec holds per page. Two pages share nothing but the website data store.
- **Side**: the question or the answer of one card. The **side HTML** is what the core renders for that side: `<style>` with the note type's CSS, followed by the rendered template (R470).
- **Update**: replacing the content of `#qa` with one side HTML and running its scripts and hooks (C6).
- **Presented**: a page is presented while it is the page the user sees. Native code marks a page presented right after the swap that brings it to the front, and unpresented when it goes back behind (U67). A page starts unpresented. A preview page is presented as soon as it is shown.
- **Page world**: the JavaScript world that card scripts run in. **Isolated world**: a separate WebKit content world that shares the DOM but not JavaScript objects with the page world. Card scripts cannot reach it.
- **Runtime**: Meguri's page-world script (`runtime.js`) plus the vendored jQuery. **Interaction script**: Meguri's isolated-world script (`interaction.js`). **Native**: Meguri's Swift side (U67).
- **Card script**: any script that is not Meguri's: scripts inside side HTML, scripts they load or create, and the scripts of `reviewExtra` (C2.3).
- **Identity** of an update: the card id and prepare generation native gives to the updates it starts (C6.8).

## C2. Page skeleton

### C2.1 Document

The page document has exactly this structure (R473, R474). Whitespace between elements is free.

```html
<!doctype html>
<html class="{html classes}" dir="ltr" data-bs-theme="{light|dark}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
<base href="{media origin root}/">
{base stylesheet (C4)}
{runtime and jQuery (C9)}
</head>
<body class="{initial body classes}">
<div id="_mark" hidden>&#x2605;</div>
<div id="_flag" hidden>&#x2691;</div>
<div id="qa" dir="auto"></div>
{reviewExtra, when set (C2.3)}
</body>
</html>
```

- `<html>` has no `lang` attribute. The document has no `<title>` (C15 D16).
- `#qa` is a direct child of `<body>`. There is no wrapper element anywhere: no `.card` div, no `#content`.
- `<base href>` is the root of the media origin with a trailing `/` (R493). U65's S2 record (`docs/design/decisions/s2-custom-scheme-media.md`) fixes the origin as `meguri-media://media`, so the element is `<base href="meguri-media://media/">`; U67 owns the constant. So `foo.jpg`, `./foo.jpg` and `/foo.jpg` in card content all resolve to the media folder.
- The page document itself is served from the media origin, under the reserved `/_anki/` prefix (R496), so its URL never equals a media file's. S2 needs no file-URL fallback. The base stylesheet, runtime and jQuery may be served there too, or be inline. A template cannot tell the difference, apart from `document.scripts` and `document.styleSheets` counts.
- There is no Content Security Policy, as a `<meta>` or a header, so `eval`, `new Function` and inline `on…` attributes work (R504).
- `data-bs-theme` is `dark` in dark appearance and `light` otherwise (C5).

### C2.2 Classes at page creation

- `<html class>`: `night-mode` first when the appearance is dark, then the platform classes (R475, set by native, never derived from the user agent): `iphone ios mobile safari` on iPhone (the foldable included), `ipad ios mobile safari` on iPad, `mac safari` on Mac. Example: iPhone in dark is `night-mode iphone ios mobile safari`.
- `<body class>` before the first side: the C3 classes without the two card classes. Examples: Mac in dark is `isMac nightMode night_mode macos-dark-mode`, iPhone in light is empty.

### C2.3 reviewExtra

When the collection config key `reviewExtra` holds a non-empty string, that HTML is placed after `#qa` as further children of `<body>` and parsed with the page (R472). Its scripts therefore run once per page, in document order, before `DOMContentLoaded`. It is never inserted again for later sides. If the key changes while a page lives, the page keeps its old copy until it is recreated. Anki desktop builds its single review page once per session, so such a script runs once per session there. With Meguri's two pages it runs twice.

### C2.4 When the first side may be inserted

Native starts the first update only after the page's `load` event has fired. So for every side, the first included:

- `document.readyState` is `complete`;
- `DOMContentLoaded` and `load` listeners that card scripts in side HTML add never fire, because both events have already happened (R468);
- `reviewExtra` scripts can still use those two events, once per page.

This matches Anki desktop in practice: its page signals readiness only after the document is ready, and its first card arrives after a round trip to the GUI process.

## C3. Body classes and the flag and mark elements

### C3.1 Body classes for a card

Native passes the body class string as the third argument of `_showQuestion` (C6.1). It is, in this order, separated by single spaces (R476):

1. `card` and `card{ord + 1}` (`card1` for the first template);
2. `isMac` on Mac;
3. `nightMode` and `night_mode` in dark appearance;
4. `macos-dark-mode` on Mac in dark appearance.

Examples: iPhone, third template, light: `card card3`. Mac, first template, dark: `card card1 isMac nightMode night_mode macos-dark-mode`. There is never `fancy`, `reduce-motion`, `no-blur`, `isWin`, `isLin` or `ankidroid_dark_mode` (C15 D19).

The class string replaces the whole `class` attribute of `<body>` during the question's update hooks (C6.4), after the side's scripts have run. So a card script running during insertion still sees the previous side's body classes. In review that is the card two back, because the pages alternate. Anki shows the same quirk with the previous card. Classes a card script added to `<body>` survive until the next question.

The answer side passes no class string, so the body classes stay as the question set them. A caller of `_showAnswer` that passes a non-empty class string sets it at the same point as the question does (Anki's previewer does this).

### C3.2 `#_flag` and `#_mark`

The two elements stay in the page for its whole life, outside `#qa`. Their content is fixed: ★ (U+2605) in `#_mark`, ⚑ (U+2691) in `#_flag`. Meguri changes them only through `_drawFlag` and `_drawMark` (C10.1, C10.2): the `hidden` attribute and the inline `color` of `#_flag`. Native calls both in a page right after it starts a card's question update there, so a page prepared beneath already shows the right flag and mark when it comes to the front, and again whenever the card's flag or mark changes. They are not queued behind updates (R478). As in Anki, the indicators therefore change before the question's content does. Native draws no second indicator. A template that hides or moves these elements with its own CSS keeps working (C4.4 notes the specificity this needs).

## C4. Base stylesheet

Meguri writes its own stylesheet (R1959) with the selectors, properties and values below. Values are exact. Selectors must keep the specificity shown, since card CSS cascades against them. The stylesheet comes before anything a side inserts, so a card rule of equal specificity wins. Inside a rule, declarations may come in any order, except that a shorthand must precede its own longhands. Rules may be grouped and ordered freely, except where two of them set the same property on the same element.

### C4.1 Colour tokens

`:root` holds the light values and `color-scheme: light`. `:root.night-mode` holds the dark values and `color-scheme: dark` (R485, R491). The value text below is exactly what a template reading the custom property gets, apart from surrounding spaces.

| Custom property | Light | Dark |
|---|---|---|
| `--fg` | `#020202` | `#fcfcfc` |
| `--fg-subtle` | `#737373` | `#858585` |
| `--fg-disabled` | `#858585` | `#737373` |
| `--fg-faint` | `#afafaf` | `#545454` |
| `--fg-link` | `#1d4ed8` | `#bfdbfe` |
| `--canvas` | `#f5f5f5` | `#2c2c2c` |
| `--canvas-elevated` | `white` | `#363636` |
| `--canvas-inset` | `white` | `#2c2c2c` |
| `--canvas-overlay` | `#fcfcfc` | `#2c2c2c` |
| `--canvas-code` | `white` | `#252525` |
| `--canvas-glass` | `rgba(255, 255, 255, 0.4)` | `rgba(54, 54, 54, 0.4)` |
| `--border` | `#c4c4c4` | `#202020` |
| `--border-subtle` | `#e4e4e4` | `#252525` |
| `--border-strong` | `#858585` | `#020202` |
| `--border-focus` | `#3b82f6` | `#3b82f6` |
| `--button-bg` | `#fcfcfc` | `rgb(25.1176470588%, 25.1176470588%, 25.1176470588%)` |
| `--button-gradient-start` | `white` | `rgb(29.0588235294%, 29.0588235294%, 29.0588235294%)` |
| `--button-gradient-end` | `#fcfcfc` | `rgb(25.1176470588%, 25.1176470588%, 25.1176470588%)` |
| `--button-hover-border` | `#999999` | `#141414` |
| `--button-disabled` | `rgba(214, 214, 214, 0.5)` | `rgba(69, 69, 69, 0.5)` |
| `--button-primary-bg` | `rgb(18.7843137255%, 41.8823529412%, 92.5490196078%)` | `rgb(15.0392156863%, 32.3333333333%, 81.0392156863%)` |
| `--button-primary-gradient-start` | `#3b82f6` | `rgb(18.3921568627%, 40.2745098039%, 88.2745098039%)` |
| `--button-primary-gradient-end` | `rgb(18.7843137255%, 41.8823529412%, 92.5490196078%)` | `rgb(15.0392156863%, 32.3333333333%, 81.0392156863%)` |
| `--button-primary-disabled` | `#93c5fd` | `rgb(26.8039215686%, 51.862745098%, 92.8039215686%)` |
| `--scrollbar-bg` | `#d6d6d6` | `#454545` |
| `--scrollbar-bg-hover` | `#c4c4c4` | `#545454` |
| `--scrollbar-bg-active` | `#afafaf` | `#636363` |
| `--shadow` | `#c4c4c4` | `#141414` |
| `--shadow-inset` | `#454545` | `#202020` |
| `--shadow-subtle` | `#737373` | `#363636` |
| `--shadow-focus` | `#6366f1` | `#6366f1` |
| `--accent-card` | `#60a5fa` | `#93c5fd` |
| `--accent-note` | `#22c55e` | `#4ade80` |
| `--accent-danger` | `#ef4444` | `#f87171` |
| `--flag-1` | `#ef4444` | `#f87171` |
| `--flag-2` | `#fb923c` | `#fdba74` |
| `--flag-3` | `#4ade80` | `#86efac` |
| `--flag-4` | `#3b82f6` | `#60a5fa` |
| `--flag-5` | `#e879f9` | `#f0abfc` |
| `--flag-6` | `#2dd4bf` | `#5eead4` |
| `--flag-7` | `#a855f7` | `#c084fc` |
| `--state-new` | `#3b82f6` | `#93c5fd` |
| `--state-learn` | `#dc2626` | `#f87171` |
| `--state-review` | `#16a34a` | `#22c55e` |
| `--state-buried` | `#f59e0b` | `#92400e` |
| `--state-suspended` | `#facc15` | `#fef9c3` |
| `--state-marked` | `#6366f1` | `#a855f7` |
| `--highlight-bg` | `rgba(37, 99, 235, 0.5)` | `rgba(147, 197, 253, 0.5)` |
| `--highlight-fg` | `black` | `white` |
| `--selected-bg` | `rgba(214, 214, 214, 0.5)` | `rgba(147, 197, 253, 0.5)` |
| `--selected-fg` | `black` | `white` |

There is no `--flag-0`.

### C4.2 Other tokens

Set on `:root`, the same in both appearances:

| Custom property | Value |
|---|---|
| `--font-size` | `15px` |
| `--border-radius` | `5px` |
| `--border-radius-medium` | `12px` |
| `--border-radius-large` | `15px` |
| `--transition` | `180ms` |
| `--transition-medium` | `500ms` |
| `--transition-slow` | `1000ms` |
| `--blur` | `20px` |
| `--focus-color` | `#6366f1` |

`--focus-color` has a second value for the Mac. The base stylesheet declares it, on every platform, for every element with class `isMac` except the document element:

- Elements: the runtime gives class `isMac` only to `<body>`, and only on the Mac (C3.1). So in practice the Mac `<body>` gets the value, and everything inside inherits it unless a rule sets the property on that element itself. `<html>` never gets it, even when a script gives it class `isMac`: it keeps `#6366f1`. A card element that carries class `isMac` itself gets the value on any platform.
- Value: the exact text `rgba(0 103 244 / 0.247)`. Templates read it as written (C4.1), so do not normalize it.
- Cascade: the declaration has specificity 0,2,0 and comes after the `:root` tokens. Card styles come later in the page than the base stylesheet. So a card rule that sets `--focus-color` on such an element replaces the value at specificity 0,2,0 or higher (for example `.card.isMac`), and loses below it (for example `.isMac`, `body.isMac`, `html body.isMac`).

This spec does not prescribe the selector. Any selector that matches exactly these elements at this specificity is correct.

`--font-size` is a token only. No rule applies it to the root (C4.5, C15 D2).

### C4.3 Document and text rules

| Selector | Property | Value |
|---|---|---|
| `*` | `box-sizing` | `content-box` |
| `html` | `font-family` | `system-ui, -apple-system, sans-serif` (C15 D1) |
| `html` | `-webkit-text-size-adjust` | `100%` (R487) |
| `body` | `color` | `var(--fg)` |
| `body` | `background` (shorthand) | `var(--canvas)` |
| `body` | `background-size` | `cover` |
| `body` | `background-repeat` | `no-repeat` |
| `body` | `background-position` | `top` |
| `body` | `background-attachment` | `fixed` |
| `body` | `margin` | `20px` |
| `body` | `overscroll-behavior` | `none` |
| `body` | `overflow-wrap` | `break-word` |
| `body.nightMode` | `background-color` | `var(--canvas)` |
| `body.nightMode` | `color` | `var(--fg)` |
| `a` | `color` | `var(--fg-link)` |
| `a` | `text-decoration` | `none` |
| `h1` | `margin-bottom` | `.2em` |
| `hr` | `background-color` | `#737373` |
| `hr` | `margin` | `1em 0` |
| `hr` | `border` | `none` |
| `hr` | `height` | `1px` |
| `img` | `max-width` | `100%` |
| `img` | `max-height` | `95vh` |
| `li` | `text-align` | `start` |
| `pre` | `text-align` | `left` |
| `button` | `margin` | `1em .5em` |

- No rule sets `font-size` anywhere, on `html`, `body`, `.card` or elsewhere. Body text is the engine default, 16 px, and `1rem` is 16 px (R486, C15 D2).
- There is no `.card` rule and no `.cloze` rule (R486). A note type's own `.card { background: white }` (specificity 0,1,0) loses to `body.nightMode` (0,1,1) in dark appearance, while `.card.nightMode` (0,2,0) beats it. That is how templates customize night colours (R488).
- `button` gets only its margin. Its look is WebKit's default (C15 D3).

### C4.4 Reviewer element rules

| Selector | Property | Value |
|---|---|---|
| `#_flag` | `position` | `fixed` |
| `#_flag` | `top` | `0` |
| `#_flag` | `font-size` | `30px` |
| `#_flag` | `-webkit-text-stroke-width` | `1px` |
| `#_flag` | `-webkit-text-stroke-color` | `#000` |
| `[dir=ltr] #_flag` | `right` | `10px` |
| `[dir=rtl] #_flag` | `left` | `10px` |
| `#_mark` | `position` | `fixed` |
| `#_mark` | `top` | `0` |
| `#_mark` | `font-size` | `30px` |
| `#_mark` | `color` | `#ff0` |
| `#_mark` | `-webkit-text-stroke-width` | `1px` |
| `#_mark` | `-webkit-text-stroke-color` | `#000` |
| `[dir=ltr] #_mark` | `left` | `10px` |
| `[dir=rtl] #_mark` | `right` | `10px` |
| `#typeans` | `width` | `100%` |
| `#typeans` | `box-sizing` | `border-box` |
| `#typeans` | `line-height` | `1.75` |
| `code#typeans` | `white-space` | `pre-wrap` |
| `code#typeans` | `font-variant-ligatures` | `none` |
| `.typeGood` | `background` (shorthand) | `#afa` |
| `.typeGood` | `color` | `#000` |
| `.typeBad` | `background` (shorthand) | `#faa` |
| `.typeBad` | `color` | `#000` |
| `.typeMissed` | `background` (shorthand) | `#ccc` |
| `.typeMissed` | `color` | `#000` |
| `.replay-button` | `text-decoration` | `none` |
| `.replay-button` | `display` | `inline-flex` |
| `.replay-button` | `vertical-align` | `middle` |
| `.replay-button` | `margin` | `3px` |
| `.replay-button svg` | `width` | `40px` |
| `.replay-button svg` | `height` | `40px` |
| `.replay-button svg circle` | `fill` | `#fff` |
| `.replay-button svg circle` | `stroke` | `#414141` |
| `.replay-button svg path` | `fill` | `#414141` |
| `.nightMode .latex` | `filter` | `invert(100%)` |
| `.drawing` | `zoom` | `50%` |
| `.nightMode img.drawing` | `filter` | `invert(1) hue-rotate(180deg)` |
| `#image-occlusion-container` | `position` | `relative` |
| `#image-occlusion-container` | `margin` | `0 auto` |
| `#image-occlusion-container` | `max-height` | `calc(95vh - 40px)` |
| `#image-occlusion-container img` | `position` | `absolute` |
| `#image-occlusion-container img` | `top`, `left` | `0` |
| `#image-occlusion-container img` | `width`, `height` | `100%` |
| `#image-occlusion-container img` | `max-width`, `max-height` | `unset` |
| `#image-occlusion-canvas` | `position` | `absolute` |
| `#image-occlusion-canvas` | `top`, `left` | `0` |
| `#image-occlusion-canvas` | `width`, `height` | `100%` |

The horizontal offsets of `#_flag` and `#_mark` sit in rules of specificity (1,1,0), an attribute selector on the direction plus the id. So a template rule `#_flag { right: 50px }` (1,0,0) does not move the flag, as in Anki, while `[dir] #_flag { right: 50px }` later in the cascade does. The page's direction is always `ltr` (C2.1), so the `rtl` rules never match. They are kept so that the cascade is the same.

### C4.5 Rules of Anki desktop's page that Meguri leaves out

| Anki desktop rule | Why it is left out |
|---|---|
| root font size from `--font-size` (15 px) | R486 keeps the engine default (C15 D2) |
| button look: background, border, radius, padding, shadow, 13 px Helvetica, weight 500, hover gradient, pressed shadow, disabled colours | desktop UI chrome; R480 keeps only the margin (C15 D3) |
| scrollbar colours and sizes on bodies without `isMac` | Windows and Linux only in Anki; Meguri's iOS body has no `isMac`, so copying the rule would wrongly apply it there |
| transitions and button shadows under `body.fancy`, the `no-blur` override | Meguri never sets those classes (C3.1) |
| `body { zoom: <app zoom> }` | card zoom is page zoom (U70) |

## C5. Night mode

Dark appearance is the app's appearance (R488). In dark appearance, all of these hold at once, and in light appearance none of them:

- `<html>` has class `night-mode` and `data-bs-theme="dark"`;
- `<body>` has classes `nightMode` and `night_mode`, plus `macos-dark-mode` on Mac;
- the `:root.night-mode` tokens and `color-scheme: dark` apply;
- `@media (prefers-color-scheme: dark)` matches inside the page, because native sets the web view's appearance (R489, U67).

No filter, inversion or colour change applies to card content except the two C4.4 rules: `.nightMode .latex` (any element with class `latex` inside the dark body, which in practice means pre-generated LaTeX images, R565) and `.nightMode img.drawing` (R492). Image occlusion mask colours come from the note type's CSS and the IO runtime (U68). The base stylesheet has no night-mode rule for them. Canvas colours are the C4.1 desktop tokens on every platform (R491).

An appearance change while a side is shown switches exactly these classes, the attribute and the web view appearance, without an update (R490, U67). Classes being added go to the end of the list: `night-mode` on `<html>`, and `night_mode`, `nightMode`, `macos-dark-mode` in that order on `<body>`. Classes being removed are removed wherever they are. Every other class stays where it is. So after a switch to dark on Mac the body reads `card card1 isMac night_mode nightMode macos-dark-mode`, as in Anki, until the next question sets the C3.1 string again. The runtime does nothing else for it.

## C6. Side updates

### C6.1 Entry points

The runtime defines these page-world functions (R516). Native starts every update by calling them by name in the page world, as Anki does. So a template that replaces one of them affects later updates in that page, as it would in Anki.

| Function | Effect | Returns |
|---|---|---|
| `_showQuestion(q, a, bodyclass)` | queues a question update (C6.2) with side HTML `q`. `a` is the answer HTML. It is used only to preload the answer's images (C7.3). `bodyclass` is the C3.1 string. | `undefined` |
| `_showAnswer(a, bodyclass)` | queues an answer update with side HTML `a`. `bodyclass` is optional. | `undefined` |
| `_updateQA(html, unused, onupdate, onshown)` | runs one update at once, not queued, with `onupdate` and `onshown` as its internal callbacks (C6.4). The second argument is ignored. | a promise that resolves when the update has finished |

Extra arguments are ignored. A call from a card script behaves the same as a call from native, except that it has no identity (C6.8).

### C6.2 Serialization

Each page keeps one queue of updates from `_showQuestion` and `_showAnswer` (R466). An update starts only when the one queued before it has finished. Finished means: its shown hooks have settled, or it was superseded (C6.6), or it failed (C6.7). So two updates never interleave, and a side's scripts never run while the previous side's hooks are still pending. `_updateQA` called directly bypasses the queue, as in Anki.

### C6.3 Steps of an update

Each update runs these steps in order (R467):

1. **Hook reset.** `onUpdateHook` and `onShownHook` are emptied in place: they stay the same array objects. Then the update's internal update callback becomes the first entry of `onUpdateHook`, and its internal shown callback the first entry of `onShownHook` (C6.4).
2. **Preload wait.** C7.1 and C7.2.
3. **Hide.** `#qa` gets the inline style `opacity: 0`.
4. **Video release.** Every `<video>` element currently inside `#qa` is paused, emptied of its child nodes (for example its `<source>` elements) and told to reload, so it lets go of its media before it is removed (R471). Other media elements are left alone. Removing them from the document in step 5 pauses them anyway.
5. **Insert.** The content of `#qa` is replaced by the side HTML, parsed as the HTML of an element. Scripts inserted this way do not run yet. `#qa` itself stays the same element: its attributes and any inline styles that card scripts gave it survive.
6. **Script walk.** C6.5.
7. **Update hooks.** `onUpdateHook` runs (C6.4) and the update waits until every value it returned has settled.
8. **Show.** `#qa`'s inline `opacity` becomes `1`.
9. **Shown phase.** When the page is presented, `onShownHook` runs (C6.4) and the update waits until every value it returned has settled. When the page is not presented, the update first settles (C6.6) and then waits for presentation.

During steps 3 to 8, card code that reads `#qa.style.opacity` sees `0`. A template's own `#qa { opacity: … }` rule never shows through after step 8, because the inline style overrides it. Both behave as in Anki.

### C6.4 Hooks

- `onUpdateHook` and `onShownHook` are page-world globals. Each is an array of functions (R509). `require("anki/reviewer")` returns the same two array objects (C10.7).
- **Running an array:** its entries are called one after another, in order, with no arguments. Each is called as soon as the previous call has returned, not after a promise it returned has settled. Functions appended to the array while it runs are called in the same run. When a call throws, the exception is caught and recorded (C12, phase `update-hook` or `shown-hook`), and the next entry is still called. Once every entry has been called, the run waits until every returned value has settled. Non-promise values count as settled, and rejections are recorded the same way. A hook failure never raises an `error` or `unhandledrejection` event in the page, so a template's own `window.onerror` does not see it, as in Anki.
- Only the original arrays are used. Assigning a new array to the global `onUpdateHook` has no effect on what runs, and neither does replacing `require("anki/reviewer").onUpdateHook`. Mutating the original array (`push`, `splice`, `length = 0`) does.
- **Internal callbacks**, always the first entries:

| Update | Internal update callback | Internal shown callback |
|---|---|---|
| question | scroll the window to the top-left corner. Then set the `class` attribute of `<body>` to `bodyclass` exactly. | remember the element with id `typeans` (null if none) for `getTypedAnswer` (C10.3). If there is one, focus it (R534). Then, without waiting, preload the answer's images (C7.3). |
| answer | if `bodyclass` is a non-empty string, set `<body>`'s `class` to it. Then, without waiting, once every `<img>` in the document has finished loading (C7.3), scroll the element with id `answer` into view, aligned to the top, if there is one. | nothing |
| `_updateQA` call | the `onupdate` argument | the `onshown` argument |

- R534 on iOS: U65's S1(e) (`docs/design/decisions/s1-occluded-page-swap.md`) found that `focus()` from page script after the swap raises the software keyboard with no tap, provided native has made the newly front web view first responder in the swap's turn (U67). The question's internal shown callback is that `focus()`: it runs at presentation (C6.6), after the swap, so no keyboard-specific code is needed in the runtime. S1(e) passed on the simulator; the device check is a U256 checkpoint.

### C6.5 Script walk

After insertion, every `<script>` element in `#qa` is made to run by replacing it with a new script element that has the same attributes (same names, same values, same order) and the same text (R467, R518, R519).

- Order: by position, in document order, over the scripts in `#qa` at the moment the walk reaches each position. A script that an earlier script adds inside `#qa` after the walk's current position is therefore also replaced, and so runs again if it already ran on insertion (case SS13). A script that an earlier script removes is skipped. When a running script removes a script at or before the walk's current position (itself included), the later scripts move down one position, so the one that moves into the current position is skipped and never runs. Anki has the same quirk, so templates do not rely on it.
- **Which elements.** Every element named `script` in `#qa`'s tree, in the HTML namespace or inside inline `<svg>`, but not inside the contents of a `<template>` element. An SVG `<script>` is replaced by an HTML script element like any other, so it runs as a classic script, and only a `src` attribute is a script source (`href` and `xlink:href` are copied but load nothing).
- **Text.** The new element's text is the old element's text content: the data of its text nodes, concatenated as the DOM holds them, never re-serialized markup. For SVG scripts this is the decoded text: `a &amp;&amp; b` in the source runs as `a && b` (C15 D21).
- **Attributes.** Copying an attribute never fails: a name the HTML parser accepted is copied as it is, even when a by-name setter such as `setAttribute` would refuse it, for example a name containing `"` or `'` (C15 D22). Attributes are copied unchanged. So the template's `type` decides how a script runs. With no `type`, or a JavaScript type, it is a classic script in sloppy mode. `type="module"` gives a module. Any other type does not run. `data-*`, `onload`, `onerror`, `async`, `defer`, `nomodule` and `crossorigin` are copied too. Meguri never adds `type`, `"use strict"` or any wrapper (R518).
- A copy made with `cloneNode` keeps the engine's "already started" state and would not run. The new element has to be created fresh.
- **Waiting:** after replacing a script, the walk goes on at once, except for a script with a `src` attribute that the engine will actually fetch. That means: no `type` or a JavaScript type and no `nomodule`, or `type="module"`. For such a script, the walk waits until the new element fires `load` or `error`, with the cap for remote scripts in C7.4. A `src` script the engine will not fetch never fires either event, so the walk does not wait for it (C15 D9).
- **Timing between scripts:** an inline classic script runs while it is being inserted. Promise jobs that a script's top-level code queues run before the next script starts. Timers, events and rendering do not happen between two scripts, unless the walk is waiting on a `src` script.
- **Isolation:** an exception thrown by a script, including a `SyntaxError` raised before any of its statements runs, stops only that script. The walk, the hooks and the update go on (R521). The exception reaches the page's normal error reporting (`window.onerror`, `error` listeners) and is recorded (C12, phase `script` with the script's position).
- `document.currentScript` is the running script element while a classic script's top-level code runs. So `document.currentScript.dataset` reads the copied `data-*` attributes. It is `null` in modules, as the platform defines.
- `onload` and `onerror` attributes on `src` scripts fire on the new element (R519).

### C6.6 Presented and unpresented pages (prepare and present)

This section exists because Meguri pre-renders the next card in the hidden page (R445, R465; Anki has a single visible page). From a template's point of view, the order of C6.3 does not change. Only the moment of step 9 does.

- **Presented page** (the answer side, a re-render of the side on screen, a preview): step 9 runs right after step 8, as in Anki.
- **Unpresented page** (a pre-rendered question): after step 8 the update **settles**, in the sense of U65's S1 record (inserted, scripts run, images decoded, fonts ready, then painted beneath). It waits until both hold, but at most 800 ms after step 8:
  - the document's fonts have no load in progress;
  - every `<img>` element in `#qa` whose `loading` attribute is not `lazy` has finished loading (it is complete, or it has fired `load` or `error`), and a decode requested for each of them has resolved or rejected.

  Then it waits for two animation frames, one requested from inside the callback of the other, so that the visible content has been painted beneath. Then it reports `settled` with its identity (C14). Then it waits.
- **Presentation**: when native marks the page presented, the runtime first resumes held media (C8.2). Then it runs the shown phase of the update that is waiting, if there is one, and reports `shown`. If presentation comes before step 8, step 9 runs right after step 8 with no settling. If it comes while the update is settling, the settling stops at once, no `settled` is reported, and step 9 runs.
- **Superseded**: an update in an unpresented page that has passed step 8 finishes without its shown phase as soon as another update is queued for this page, or at once when one was queued before it passed step 8. If it was still settling, it reports no `settled`. Its internal shown callback does not run either, so `getTypedAnswer` and focus keep their previous state. Then the next update starts.
- `#qa` has opacity `1` in a settled hidden page, so the swap shows the finished side.

### C6.7 Failed updates

When a step of the update itself fails, as opposed to a card script inside it (for example because an earlier card script removed `#qa`, so there is nowhere to insert), the update stops. The runtime reports `updateFailed` with the identity and the error text (C14), and the next queued update still starts (C15 D11). Native's recovery, recreating the page, is U70's.

### C6.8 Identity of updates

Native gives each update it starts an identity: card id and prepare generation (U67). Every signal the runtime sends about an update carries it (C14). The identity must reach the update even when a card script has wrapped `_showQuestion` or `_showAnswer` and calls the original with only Anki's arguments. So it cannot travel as an extra argument. Native announces it through the internal channel before the call, and the runtime attaches it to the next update that is queued. Updates that card scripts start themselves have no identity. Their signals are sent without one, and native ignores them.

## C7. Preloading and waits

### C7.1 What the preload step loads

Before inserting a side (C6.3 step 2), the runtime parses the side HTML in an inert template, so that nothing in it runs or loads, and starts loading three kinds of resources:

- **images:** every `img` element with a `src` attribute, resolved against the page base. Each is loaded through a detached image with asynchronous decoding, and a decode is requested whose failure is ignored;
- **stylesheets:** every `<link rel="stylesheet">`, and every `<style>` whose text contains `@import`. A copy of each is put in `<head>` with `media="print"`, so that it cannot change rendering, and is removed again on `load` or `error`;
- **fonts:** for every `<style>` in the side, its CSS is parsed without affecting the page, and every `url(…)` source of every top-level `@font-face` rule is taken (`local(…)` sources are skipped), resolved against the page base. A URL this page has already asked for is skipped. The page remembers every font URL it asks for, for its whole life, even when the load failed. Each new URL is loaded through a temporary `<link rel="preload" as="font" crossorigin>` in `<head>`, removed again on `load` or `error`.

`srcset`, CSS backgrounds, `<image>` in SVG, media elements and scripts are not preloaded.

### C7.2 Budget

One wait per update (R449):

- nothing of the three kinds: no wait;
- otherwise the update waits until every started load has finished, or until the budget runs out, whichever comes first. The budget is **800 ms** if any new font was started, else **500 ms** if any stylesheet was started, else **200 ms** (images only).

Loads still running when the budget ends go on in the background.

### C7.3 Answer images

From the question's internal shown callback, without delaying anything: once every `<img>` element in the document has finished loading (it is complete, or it has fired `load` or `error`), the images of `a`, the second argument of `_showQuestion`, are requested the same way as in C7.1, with no wait (R443).

### C7.4 Remote scripts

R499: remote loads never delay presentation beyond the preload budgets. During one script walk, the time spent waiting on `src` scripts whose URL is not on the media origin adds up to at most **800 ms**. After that, the walk no longer waits for remote scripts in that update. A remote script that arrives later still runs when it arrives, and its `onload` still fires. Scripts on the media origin are always waited for (C15 D10).

### C7.5 Cache warming

Native can ask the runtime to warm the page's caches with any side HTML, for example the answer, or cards further down the queue (U175 decides when; R443, R444). This loads the C7.1 resources without inserting anything and without waiting. A warming request is not a page global (C14).

## C8. Media elements and presentation

### C8.1 At insertion

C6.3 step 4 applies to every update.

### C8.2 Holding playback in an unpresented page

A hidden page must not be heard. While a page is unpresented:

- any `<audio>` or `<video>` in the document that starts playing, because of `autoplay` or a script's `play()`, is paused at once and remembered;
- when the page becomes presented, every remembered element that is still in the document is played again from where it stopped. A refused `play()` is ignored. The remembered list is cleared at every insertion (C6.3 step 5).

When a page stops being presented (it goes behind at a swap, R560), every `<audio>` and `<video>` in it is paused. These are not remembered: that card is over.

Not held: Web Audio, the Web Speech API, timers and animations. A pre-rendered side that uses them starts them while hidden (C15 D7, C19).

## C9. Page-world globals

### C9.1 The allowlist

These are the only own properties of `window` that Meguri's page-world scripts add (R510, R512–R517):

| Global | Mac | iOS, iPadOS | Value | Defined as |
|---|---|---|---|---|
| `$`, `jQuery` | yes | yes | jQuery 3.7.1, the same function | assignment |
| `anki` | yes | yes | object, C10.6 | assignment |
| `ankiPlatform` | yes | no | the string `"desktop"` (R513) | assignment |
| `require` | yes | yes | function, C10.7 | assignment |
| `onUpdateHook`, `onShownHook` | yes | yes | arrays, C6.4 | assignment |
| `_showQuestion`, `_showAnswer`, `_updateQA` | yes | yes | functions, C6.1 | assignment |
| `_drawFlag`, `_drawMark` | yes | yes | functions, C10.1, C10.2 | assignment |
| `getTypedAnswer` | yes | yes | function, C10.3 | assignment |
| `_typeAnsPress` | yes | yes | function, C10.4 | assignment |
| `MathJax` | yes | yes | object, C10.8 | assignment |
| `pycmd`, `bridgeCommand` | yes | no | the same function, C10.5 | as by `var` |

- **Assignment** means a data property that is writable, enumerable and configurable, as an ordinary `window.x = …` creates. A card script may then reassign it, `delete` it, or shadow it with a top-level `let`, `const` or `class` without error.
- **As by `var`** means writable and enumerable but not configurable. A card script's top-level `let pycmd` or `const pycmd` throws a `SyntaxError` on Mac, as on Anki desktop, while `var pycmd = …` and `function pycmd() {}` reassign it.
- All of these exist before any card script, `reviewExtra` included, runs.
- Meguri's scripts create no global lexical bindings (top-level `let`, `const` or `class` in a classic script). Such a binding would not show up as a property of `window`, but a card script declaring the same name would throw. Meguri's scripts also create no other `var` or function globals.
- Image occlusion (U68) adds `anki.imageOcclusion` and `anki.setupImageCloze` as properties of `anki`, not as globals.

### C9.2 Names that are never defined

Never defined in the page world (R517): `AnkiDroidJS`, `showAnswer`, `buttonAnswerEase1`, `buttonAnswerEase2`, `buttonAnswerEase3`, `buttonAnswerEase4`, `study`, `setInnerHTML`. Also left out (R516) are the other globals of Anki desktop's page: `_queueAction`, `_emulateMobile`, `_blockDefaultDragDropBehavior`, and the Qt bridge objects `qt` and `QWebChannel`. On iOS and iPadOS, `pycmd`, `bridgeCommand` and `ankiPlatform` are undefined (R505, R513).

### C9.3 Message handlers visible to card scripts

- iOS and iPadOS: `window.webkit.messageHandlers.cb` exists. It is AnkiMobile's handler, and U67 maps its messages (R511). No other handler that Meguri registers can be reached from the page world.
- Mac: the page world has no `window.webkit` at all, as on Anki desktop, which is not WebKit. So templates that look for `window.webkit` to detect iOS do not take their iOS path on Mac. `pycmd` reaches native through the internal channel (C14).

### C9.4 How the surface is checked

For each platform, load the same skeleton in a web view with the same configuration but without the runtime and jQuery. Compare `Object.getOwnPropertyNames(window)` of the card page with that baseline:

- the names the card page has and the baseline lacks must equal the C9.1 set for that platform exactly;
- the names the baseline has and the card page lacks must be none.

Run the comparison on a fresh page and again after several sides whose scripts use jQuery and the hooks. Check the C9.2 names separately, and `typeof webkit` on Mac. Check the absence of global lexical bindings statically: the top level of `runtime.js` is a single expression statement that calls a function expression.

The runtime may need to wrap built-in prototype methods for the listener registry (C13). That changes no global name, but `EventTarget.prototype.addEventListener` is then no longer native code when inspected. This is accepted.

## C10. Function semantics

### C10.1 `_drawFlag(flag)`

`#_flag` gets the `hidden` attribute exactly when `flag` is the number `0`, and loses it otherwise. Its inline `color` becomes `var(--flag-` + the argument as text + `)`. That is set for `0` too, which names an undefined token while hidden. Returns `undefined`.

### C10.2 `_drawMark(mark)`

`#_mark` gets `hidden` when `mark` is falsy, and loses it otherwise. Returns `undefined`.

### C10.3 `getTypedAnswer()`

Returns the current `value` of the element the most recent question's internal shown callback remembered (C6.4), or `null` when it remembered none, when no question has been shown in this page yet, or when that element's `value` is `undefined` or `null` (an element with id `typeans` that is not a form control). The element is not looked up again. On the answer side, after the question's `#typeans` was replaced, it still returns the detached input's value.

Native reads the typed answer by calling the page's `getTypedAnswer` by name, before it starts the answer update. A string is the typed text. `null` or anything else counts as empty. A throw counts as empty and is recorded (R536; a template that overrides it supplies its own typed text, as in Anki).

### C10.4 `_typeAnsPress()`

Defined on every platform (R508). Meant as a key handler: `#typeans` calls it from `onkeypress`, and templates call it from their own handlers.

- It reads `window.event`. When there is no current event, it throws a `TypeError`, as in Anki.
- When the event's `key` is not `Enter`, it does nothing.
- When `key` is `Enter`, the page's current side is a question (the side of the latest update in this page), and the Enter is not part of IME input (below), it requests a reveal through the internal channel (C14). It does not go through the page's `pycmd` or `bridgeCommand`, which do not exist on iOS (C15 D14). It works the same on cards without `{{type:}}`.
- It returns `undefined`.

**IME rule (R535, Meguri behavior).** From its start, the runtime tracks composition and keys on `window` in the capture phase, ahead of every card listener. An Enter is ignored when any of these holds:

1. a composition is in progress: `compositionstart` was seen and no `compositionend` since;
2. the event itself has `isComposing` true or `keyCode` 229;
3. the most recent `keydown` had `isComposing` true or `keyCode` 229, and no `keyup` has come since (it is the same keystroke);
4. a `compositionend` was seen, and since then there has been neither a `keyup` nor a `keydown` whose `keyCode` is not 229 and whose `isComposing` is false.

Rule 4 covers WebKit's order, where the keystroke that commits a composition delivers `compositionend` before its own `keydown`. The keystroke after it reveals normally. The resulting sequences:

| Events, in order | Reveal |
|---|---|
| `keydown` Enter (13), `keypress` Enter | yes |
| `compositionstart`, `keypress` Enter | no (1) |
| `compositionstart`, `keydown` (229), `compositionend`, `keypress` Enter | no (3) |
| `compositionstart`, `compositionend`, `keydown` (229), `keypress` Enter | no (3, 4) |
| `compositionstart`, `compositionend`, `keydown` Enter (13, not composing), `keypress` Enter | yes |
| `compositionstart`, `compositionend`, `keyup`, `keydown` Enter (13), `keypress` Enter | yes |

The runtime's own tracking listeners do not count as card listeners (C13).

### C10.5 `pycmd(command, callback)` and `bridgeCommand` (Mac only)

`pycmd` and `bridgeCommand` are the same function (R506, R507):

- `command` is converted to a string. It goes to native through the internal channel, with the identity of the page's current side.
- It returns `false` synchronously, so that `onclick="return pycmd(…)"` cancels the click's default action.
- When `callback` is a function, it is called exactly once, in a later task, never synchronously, with the single argument `null`, for every command. That is the value Anki desktop's bridge delivers for every reviewer command (C19 item 1, settled: `null`, not R507's `undefined`). A non-function callback is ignored.
- Native's handling (U67 allowlist, R2038):

  | Command | Effect |
  |---|---|
  | `ans` | on a presented question: reveal the answer. From a page not yet presented: queued and replayed when that same card is presented (U67). On an answer side: ignored (C15 D17) |
  | `play:q:N`, `play:a:N` (N a decimal integer) | stop the audio queue and play the Nth AV tag of that side |
  | anything else (`ease1`–`ease4`, `edit`, `more`, `domDone`, `close`, add-on commands such as `play_audio-…`, `update_card_type|…`) | ignored and logged, never an exception in the page (C15 D4) |

### C10.6 `anki`

A plain object. U66 defines one member:

- `anki.mutateNextCardStates(key, transform)`: returns a promise that is already resolved with `undefined`. It never calls `transform`, never rejects, and never delays the answer buttons (R512; custom scheduling is out of scope).

U68 adds `anki.imageOcclusion` and `anki.setupImageCloze`. There are no other members.

### C10.7 `require(name)` and packages

| Name | Platforms | Value |
|---|---|---|
| `anki/packages` | all | `{require, listPackages, hasPackages}` |
| `anki/bridgecommand` | Mac | `{bridgeCommand}`, a function `(command, callback)` that calls the page's current global `bridgeCommand`, looked up at call time, with those two arguments, and returns `undefined` (not the global's `false`) |
| `anki/reviewer` | all | `{onUpdateHook, onShownHook}`: the same array objects as the globals (C6.4) |

- `require(name)` returns the registered object itself (a plain, mutable object; the same object on every call). For any other name it throws an `Error` whose message is `Cannot require "` + name + `" at runtime.`. On iOS this includes `anki/bridgecommand`. Names that every object inherits, such as `toString` or `constructor`, are not registered names: `require` throws for them and `hasPackages` returns `false` (C15).
- `listPackages()` returns a new array of the registered names, in this order: `anki/packages`, then `anki/bridgecommand` (Mac only), then `anki/reviewer`.
- `hasPackages(...names)` returns `true` when every given name is registered, including when no name is given.
- `require("anki/packages").require` is the global `require`.

### C10.8 `MathJax`

A plain object (R515, R563):

- `MathJax.typeset(elements)` returns `undefined` and changes nothing;
- `MathJax.typesetPromise(elements)` returns an already-resolved promise and changes nothing;
- `MathJax.typesetClear(elements)` returns `undefined`;
- `MathJax.startup.promise` is an already-resolved promise.

It exists from the start in every page. Meguri never loads MathJax and never typesets. `\(…\)` and `\[…\]` stay as plain text in the side (C15 D6).

### C10.9 jQuery

The upstream jQuery 3.7.1 release (MIT), unmodified, so `$.fn.jquery === "3.7.1"` and `$ === jQuery` (R514). It is not Anki's copy (R1960). It is listed in `THIRD_PARTY_NOTICES.md`. A card that loads another jQuery replaces the globals, as it would in Anki.

## C11. Script semantics templates rely on

Within one page (R468, R469):

- Window globals, `document` and `window` listeners, timers, the `<body>` element with its listeners and inline styles, and `#qa` with its attributes all survive from side to side and from card to card in the same page. In review that means every other card (R465).
- A top-level `let`, `const` or `class` declaration that a classic script repeats on a later side throws a `SyntaxError` before any of that script's statements runs. Templates avoid this with `var` or a wrapping function, as they must in Anki. A `var` counter increments on every side and every card in the page.
- `DOMContentLoaded` and `load` happen once per page (C2.4).
- Sloppy mode: assigning to an undeclared name creates a global, and `this` at the top level is `window`.
- Dynamic `import()` of media-folder `.js` files works with relative (`./_m.js`) and root-absolute (`/_m.js`) specifiers. Both resolve against the page base (R520; MIME and CORS are U67's).
- The answer replaces the question in `#qa`. The core renders the answer's `{{FrontSide}}` as the question HTML without its `<style>`, so the question's scripts run again on the answer side, and its inputs are new elements: the typed text is gone (R469).
- `sessionStorage` lasts as long as the page, and `localStorage` and IndexedDB are shared and persistent (U67, R502, R503).

## C12. Errors and diagnostics

Everything below is recorded for the card's diagnostics, the "Template Errors…" view (R522, U67, U175). Recording never changes what the page sees: the runtime never cancels, stops or swallows a page `error` or `unhandledrejection` event.

| Field | Content |
|---|---|
| `kind` | `exception` (uncaught), `rejection` (unhandled promise rejection) or `hook` (caught by the runtime, C6.4) |
| `message` | for an `Error`: its name, `: ` and its message (`TypeError: null is not an object (evaluating 'null.x')`). Otherwise the value converted to a string, or `<unprintable>` when that conversion throws. At most 2,000 characters |
| `line`, `column` | as the engine reports them (for an inline script, WebKit counts from the start of that script's own text, line 1). Absent for rejections and hooks |
| `source` | the script URL for a `src` script or module, otherwise `inline` |
| `phase` | `script` with `index`, the 1-based position of the script in the walk, when the exception is thrown while that script's top-level code runs or compiles; `update-hook` or `shown-hook`; otherwise `later` (event handlers, timers, promise jobs after the walk) |
| `side` | `question` or `answer`: the side of the update that started most recently in this page |
| identity | of that update (C6.8) |

- Uncaught exceptions and unhandled rejections come from any page-world code: card scripts, `reviewExtra`, event handlers, timers, and code Meguri calls such as card-supplied hooks (R522).
- Reading a rejection's reason may only work from the page world. The runtime is responsible for the `rejection` text.
- Failed media loads are recorded separately (C14 `mediaError`, R498).

## C13. Listener registry (Meguri)

Meguri's tap and key handling (U180, U181) acts quickly only when no card listener could see the input. The runtime keeps the register that tells it so. Anki has no counterpart.

- **Event types.** Tap set: `touchstart`, `touchend`, `pointerdown`, `pointerup`, `mousedown`, `mouseup`, `click`. Key set: `keydown`, `keypress`, `keyup`.
- **What counts as a card listener** for type T on target X (`window`, `document` or any element):
  - a listener added with `addEventListener(T, …)` on X by page-world code that is not Meguri's, in either phase, until it is removed (`removeEventListener` with the same type, callback and capture flag, or its `signal` aborting). Adding the same type, callback and capture twice counts once. A `null` callback does not count. A `once` listener may keep counting after it fired: counting too much is allowed (rule below);
  - a non-null `on{T}` event handler on X, whether set as a property (`document.onkeydown = f`) or as an attribute (`<div onclick="…">`), present at insertion or added later.
- **Lifetime.** Listeners on `window`, `document`, `<html>` and `<body>` persist across sides and cards in the page. A listener on any other element counts only while that element is connected to the document, so element listeners go away with their side.
- **Question answered.** For an input of type T aimed at element E (for keys, the focused element, or `<body>` when nothing is focused): is there a card listener for T on `window`, on `document`, or on E or any of its ancestors? Ancestors are followed across shadow roots to their hosts.
- **Never under-report.** When unsure, the answer is yes. Saying yes wrongly only costs the fast path. Saying no wrongly would let Meguri act on an input the card handles.
- **Exclusions.** Listeners that Meguri's own page-world code adds (the IME tracking of C10.4, the media holding of C8.2) do not count. Isolated-world listeners are invisible to the registry anyway.
- **Availability.** The answer must be available to Meguri's isolated-world scripts without a page global. Either the state is published with the hit map after each update and after each change (U176, U180), or each query is answered through the internal channel. Handler properties may be read at query time rather than intercepted.

## C14. Internal channel and signals (Meguri)

- Meguri's runtime, interaction script and native exchange messages without any page global (C9.1) and without a page-world message handler other than iOS `cb` (C9.3). For example, they can use DOM events whose payload is a string, dispatched on the document between the page world and the isolated world, with native talking only to the isolated world.
- **Trust.** Card scripts may be able to observe or imitate the runtime's internal events. Native therefore treats everything that comes from the page world as untrusted and allows only the card-scoped actions of R2038: reveal, play an AV tag of the current card, the typed answer, the `cb` tap zones, and Meguri's diagnostic and state signals. A forged signal may at worst mistime or misreport the forging page's own card. It can never act on another card, on the collection or on a native API.
- **Signals.** The names are this spec's. U67 fixes the message format and the allowlist (`BridgeAllowlistTests`). Every signal about an update carries its identity (C6.8).

| Signal | From | When | Content |
|---|---|---|---|
| `settled` | runtime | C6.6, unpresented page | side |
| `shown` | runtime | after the shown phase | side |
| `updateFailed` | runtime | C6.7 | error text |
| `visible` | interaction | two animation frames after the page is presented and the current update has passed step 8 (R1862) | side |
| `command` | runtime | `pycmd` on Mac (C10.5); a reveal request from `_typeAnsPress` (C10.4) | command text, or reveal |
| `error` | runtime or interaction | C12 | the C12 fields |
| `mediaError` | interaction | an `img`, `audio`, `video`, `source`, `link`, `object`, `embed` or `script` element fails to load (an `error` event, captured on `window`), or the document's font set reports a failed font load | element tag, URL, side (R498). Native's media handler records 404s for CSS-loaded resources (U67) |
| `selection` | interaction | the selection changes between empty and non-empty | whether a non-collapsed selection exists (R406) |
| `focus` | interaction | focus enters or leaves an editable element: `input`, `textarea` or contenteditable | editable or not, whether it is `#typeans` (R425) |
| `composition` | interaction | composition starts or ends | active or not (R535) |
| `typedText` | interaction | `input` events on `#typeans` | its current value (for R538 continuity, U70; the answer still uses C10.3) |
| `listeners` | runtime | C13, when published as state | the registry answer or state |

- Native may also send these to the page: the identity of the next update (C6.8), presented and unpresented (C6.6, C8.2), and warm caches (C7.5).

## C15. Deliberate differences from Anki desktop

Each difference has its own test (R1889). The test names are suggestions in the U66 suites. The plan's known-deviations list for oracle goldens (`conformance/deviations.yaml`) has no form for page behavior, which the oracle cannot observe. This table is that list for the card runtime.

| Id | Requirement | Anki desktop | Meguri | Test |
|---|---|---|---|---|
| D1 | R479 | `html` font is `"Helvetica"` (Mac) | `system-ui, -apple-system, sans-serif` | `BaseCSSTests.htmlFontIsSystemStack` |
| D2 | R486 | root font size is `var(--font-size)`, 15 px, so body text and `1rem` are 15 px | engine default, 16 px; `--font-size` is still defined | `BaseCSSTests.rootFontSizeIsEngineDefault` |
| D3 | R480 | card `<button>`s get the desktop UI button look | WebKit's default button look plus the margin | `BaseCSSTests.buttonKeepsPlatformAppearance` |
| D4 | R506 | `ease1`–`ease4`, `edit`, `more` act | ignored and logged | `GlobalsSurfaceTests.pycmdIgnoresOtherCommands` |
| D5 | R513, R505 | `ankiPlatform`, `pycmd`, `bridgeCommand` exist | iOS and iPadOS: undefined (AnkiMobile's environment) | `GlobalsSurfaceTests.iosHasNoDesktopBridge` |
| D6 | R515, R563 | MathJax is loaded when a side contains `\(…\)` or `\[…\]` and typesets it; `MathJax` is undefined before that | never loaded; the stub always exists; source text stays | `GlobalsSurfaceTests.mathJaxStubResolves`, `GlobalsSurfaceTests.mathSourceStaysVisible` |
| D7 | R445, R465 | a side's scripts run when it is displayed | a pre-rendered question's scripts run while hidden, and its shown hooks at display; media is held (C8.2); Web Audio, speech and timers are not held | `SideUpdateTests.shownHooksWaitForPresent`, `SideUpdateTests.autoplayHeldUntilPresent` |
| D8 | R516 | `_queueAction`, `_emulateMobile`, `_blockDefaultDragDropBehavior` are globals | not defined | `GlobalsSurfaceTests.globalsMatchAllowlist` |
| D9 | R467 | waits for `load`/`error` of every `src` script, so a non-fetched `src` script (unknown `type`, `nomodule`) stops the reviewer for good | does not wait for scripts the engine will not fetch | `ScriptSemanticsTests.unfetchedScriptDoesNotBlock` |
| D10 | R499 | waits for a remote `src` script however long it takes | at most 800 ms of remote waiting per walk | `ScriptSemanticsTests.remoteScriptWaitIsCapped` |
| D11 | R466 | after a failed update, no later update runs | the failure is reported and later updates run | `SideUpdateTests.failedUpdateDoesNotBlockQueue` |
| D12 | R522 | hook failures go to the console only | also recorded in diagnostics (not visible to the page) | `SideUpdateTests.hookFailureRecorded` |
| D13 | R535 | any Enter keypress reveals | Enter that belongs to IME input does not (C10.4) | `TypeInRuntimeTests.imeEnterDoesNotReveal` |
| D14 | R508 | `_typeAnsPress` goes through the page's current `bridgeCommand` | through Meguri's internal channel, on both platforms | `TypeInRuntimeTests.enterRevealsWithoutBridge` |
| D15 | R473, R487 | no viewport meta, no `-webkit-text-size-adjust` | both present | `BaseCSSTests.viewportAndTextSizeAdjust` |
| D16 | R473 | page title `main webview` | no `<title>` (`document.title` is empty) | `GlobalsSurfaceTests.documentHasNoTitle` |
| D17 | R506 | `pycmd("ans")` on the answer side shows the answer again (scripts rerun) | ignored | `GlobalsSurfaceTests.ansOnAnswerIgnored` (native part in U67) |
| D18 | R445 | no font or image wait before display beyond the preload budget | an unpresented page waits up to 800 ms for fonts and for the images in `#qa` to load and decode before it settles (C6.6) | `SideUpdateTests.settleWaitsForFonts`, `SideUpdateTests.settleWaitsForImageDecode` |
| D19 | R476 | body also has `fancy` (unless minimalist mode) | never | `NightModeTests.bodyClassesExact` |
| D20 | R475 | `<html>` platform classes come from the user agent, `mac chrome` on Mac | set natively, `mac safari` on Mac | `NightModeTests.htmlClassesExact` (U67 sets them) |
| D21 | R467 | a script's text is copied by re-serializing it, so an SVG `<script>` whose text holds `&`, `<` or `>` reaches the engine with entities (`&amp;`) and fails to compile | the text content is copied, so such a script runs (C6.5) | `ScriptSemanticsTests.svgScriptRunsWithDecodedText` |
| D22 | R467 | attributes are copied by name, so a name the engine's by-name setter refuses stops the update and shows an error message in place of the side | every parsed attribute is copied and the side runs (C6.5) | `ScriptSemanticsTests.unusualAttributeNameIsCopied` |

Also not reproduced, with no template-visible effect expected: Anki desktop's Backspace guard on `document`, its IME focus workaround (a temporary invisible button on `focusout`) and its drag-and-drop blocker on `document` (`ondragenter`, `ondragover`, `ondrop` set). Navigation on drop is U70's.

Two edge cases also differ, with no template-visible effect expected: Anki desktop calls a `pycmd` callback that is truthy but not a function, which then throws in a later task, where Meguri ignores it (C10.5); and Anki's `require` and `hasPackages` treat names that every object inherits (`toString`, `constructor`, …) as registered, where Meguri throws and returns `false` (C10.7, case GS4c).

## C16. Verification cases

Synthetic content only: no Anki templates or CSS (R1959, R1960), and no Migaku code or user content (R1915). Each case lists inputs and what the test observes. The side HTML is given as the test passes it to `_showQuestion` or `_showAnswer`. Media files are written into the test's media folder. `log(x)` stands for appending `x` to a page-world array the test reads, `window.__log`, created by the case's first script.

### C16.1 SideUpdateTests (R466, R467, R471, R472, C6–C8)

| # | Input | Observed |
|---|---|---|
| SU1 | unpresented page. Question: script 1 inline: `log("A")`, pushes update hook `U` (logs `U-start`, the body class, `#qa` opacity and `scrollY`, then returns a promise resolving 50 ms later after logging `U-end`) and shown hook `S` (logs `S`). Script 2: `src="_su_b.js"` (file logs `B`). Script 3 inline: logs `C` and whether `B` already logged. Body class `card card2` | log is `A, B, C(true), U-start(card card2, 0, 0), U-end` before `settled`; `settled` comes after `U-end` and after `#qa` opacity is `1`; `S` absent until the page is presented, then `S` and `shown` |
| SU2 | presented page, answer side with a shown hook | the shown hook runs without waiting for presentation; `shown` follows |
| SU3 | `_showQuestion` called twice quickly on a presented page; each side's script logs its own name; each pushes a shown hook | the second side's script logs only after the first side's shown hook |
| SU4 | unpresented page: question Q1 settled; then `_showQuestion(Q2)` before presentation | Q1's shown hook never runs; Q2 inserts; `getTypedAnswer()` unchanged |
| SU5 | side 1 has `<video autoplay muted><source src="_v.mp4"></video>` and a script that keeps the element in `window.__v`; then side 2, whose first script logs `__v.paused` and `__v.childElementCount` | `true`, `0`; an `emptied` event reached `__v` afterwards |
| SU6 | `reviewExtra` = `<div id="rx"></div><script>window.__rx=(window.__rx\|\|0)+1</script>`; three sides | `#qa.nextElementSibling.id === "rx"`; `__rx === 1` |
| SU7 | unpresented page, question with `<audio src="_tone.m4a" autoplay loop>` | `paused` is true and no audible playback until presentation, then `paused` is false; after the page is unpresented again, `paused` is true |
| SU8 | question whose script removes `#qa`, then another `_showQuestion` | two `updateFailed` signals, the second proving the queue went on |
| SU9 | hook that throws and hook that rejects; page `window.onerror` set by the card | two `hook` diagnostics; the card's `onerror` was not called; next hooks ran |
| SU10 | unpresented page, side with an `@font-face` (`_f.woff2`, `font-display: block`) used by visible text | `settled` comes after `document.fonts.check("12px F")` is true (or after 800 ms) |
| SU11 | side with `<img src="_big.png">` only; the server delays it 1 s | insertion happens about 200 ms after the call |
| SU12 | listener registry: (a) side script adds `keydown` on `document`; (b) side with `<div id=t onclick="">` and a sibling `<p>`; (c) plain side; (d) `document.onkeydown = f`, then `null`; (e) card N adds a `touchstart` listener on `window`, card N+1 in the same page has no script, then the listener is removed | (a) key: yes; (b) tap on `#t`'s child: yes, on the `<p>`: no; (c) no for both sets; (d) yes, then no; (e) yes on N+1, no after removal; Meguri's IME tracking never counts |
| SU13 | unpresented page; (a) side with `<img src="_slow.png">` that the server answers after 600 ms; (b) side with only `<img loading="lazy" src="_never.png">` 5,000 px below the top, whose request never answers; (c) side with only `<img src="_never.png">`, never answered | (a) when `settled` arrives, the image is `complete` with `naturalWidth` > 0; (b) `settled` comes well under 800 ms after step 8; (c) `settled` comes about 800 ms after step 8 |
| SU14 | unpresented page, side as SU13 (c) with a shown hook; the page is presented 100 ms after step 8 | no `settled` for that update; the shown hook runs at once and `shown` follows |

### C16.2 ScriptSemanticsTests (S9, S10; R468, R469, R503, R518–R521)

| # | Input | Observed |
|---|---|---|
| SS1 | two sides, each `<script>const x = 1; window.__x = (window.__x\|\|0) + 1;</script>` | after side 2, `__x === 1`; a `SyntaxError` diagnostic with phase `script`, index 1, side of the second update |
| SS2 | every side: `<script>var n = (typeof n === "number" ? n : 0) + 1; log(n)</script>`, five sides, one page | log `1,2,3,4,5` |
| SS3 | side script adds `DOMContentLoaded` and `load` listeners and logs `document.readyState` | logs `complete` on every side, the first included; neither listener ever runs |
| SS4 | `<script data-k="v">log(document.currentScript.dataset.k)</script>` | `v` |
| SS5 | `<script src="_missing.js" onerror="log('E')"></script><script>log('next')</script>` | `E, next` |
| SS6 | `<script src="_lib.js"></script><script>log(typeof window.__lib)</script>` (file sets `__lib`) | `number` |
| SS7 | script 1 `null.x`; script 2 `log(2)` | `2`; one `exception` diagnostic, phase `script`, index 1, line 1 |
| SS8 | script 1: `Promise.resolve().then(()=>log("m")); setTimeout(()=>log("t"))`; script 2: `log("s2")` | `m, s2` before `t` |
| SS9 | `<script>import('./_m.js').then(m=>log(m.v)); import('/_m.js').then(m=>log(m.v))</script>` | two values logged |
| SS10 | question `<input id=fi><script>log(document.getElementById('fi'))</script>`, answer = the same HTML plus `<hr id=answer>` | the script logged twice; the two inputs are different elements |
| SS11 | `<script>undeclared = 5; log(this === window)</script>` | `window.undeclared === 5`, `true` |
| SS12 | `<script type="text/x-tmpl" src="_t.js"></script><script nomodule src="_n.js"></script><script>log("after")</script>` | `after` without waiting for either |
| SS13 | script 1 appends `<script>window.__d=(window.__d\|\|0)+1</script>` (created with `createElement`, so it runs on insertion) to the end of `#qa` | `__d === 2` after the walk |
| SS14 | `<script type="module">log(document.currentScript)</script>` | `null` is logged (a module ran) |
| SS15 | `<script src="{other origin}/slow.js"></script><script>log("x")</script>`, where the other origin is not the media origin and the test makes it never answer (for example a second test scheme handler that holds the request) | `x` logged at most about 800 ms after the walk started |
| SS16 | `<svg><script>window.__s = 1 &amp;&amp; 2; log("svg")</script></svg>` | `svg` logged; `__s === 2` |
| SS17 | `<script a"b="v">log(document.currentScript.getAttribute('a"b'))</script><script>log("next")</script>` | `v, next`; no `updateFailed` |
| SS18 | `<template><script>window.__t = 1</script></template><script>log(typeof window.__t)</script>` | `undefined` |
| SS19 | S9, a persistence round trip in the style of the community persistence scripts, written for the test: the question logs `typeof sessionStorage`, `typeof py` and `typeof qt`, then stores `k` = `v` in `sessionStorage`; the answer logs `sessionStorage.getItem("k")`; then a second page (as for the next card) logs the same item on its first question | `object`, `undefined`, `undefined`; `v`; `null` (R503) |

### C16.3 GlobalsSurfaceTests (S15; R505–R517, R563, R2038)

| # | Check | Expected |
|---|---|---|
| GS1 | C9.4 diff on Mac and on iPhone, fresh and after five sides | exactly the C9.1 set for the platform |
| GS2 | `typeof` each C9.2 name | `undefined` |
| GS3 | card script `let $ = 1; log($)` on a later side; on Mac `let pycmd = 1` | `1`; on Mac a `SyntaxError` |
| GS4 | `require("anki/reviewer").onShownHook === onShownHook`; `require("x")`; `require("anki/packages").listPackages()` | `true`; throws `Cannot require "x" at runtime.`; the C10.7 order |
| GS4b | Mac, on a presented question: `require("anki/bridgecommand").bridgeCommand("ans", cb)`; then the card sets `window.bridgeCommand` to a function that logs its arguments and returns `7`, and calls the package function with `"x"` only | returns `undefined`, a `command` signal `ans`, `cb` called once, later, with `null`; returns `undefined`, and the card function logged `x` and `undefined` |
| GS4c | `require("toString")`; `hasPackages("constructor")`; `hasPackages()` | throws `Cannot require "toString" at runtime.`; `false`; `true` |
| GS5 | Mac: `pycmd("ans", cb)` on a presented question | returns `false`; a `command` signal `ans`; `cb` called once, later, with `null` |
| GS6 | Mac: `pycmd("ease3")`, `pycmd("play:q:1")` | no exception; `command` signals passed to native, which ignores `ease3` |
| GS7 | iPhone: `typeof pycmd`, `typeof bridgeCommand`, `typeof ankiPlatform`, `require("anki/bridgecommand")` | `undefined` ×3; throws |
| GS8 | Mac: `ankiPlatform`, `typeof webkit` | `"desktop"`, `"undefined"` |
| GS9 | `await MathJax.typesetPromise()`, `await MathJax.startup.promise`, `MathJax.typeset()` | resolve, resolve, `undefined` |
| GS10 | side `<p>\(x^2\) and \[y\]</p>` | the paragraph's text is exactly `\(x^2\) and \[y\]` after the update |
| GS11 | `$.fn.jquery`, `$ === jQuery` | `"3.7.1"`, `true` |
| GS12 | `anki.mutateNextCardStates("k", t)` with a spy `t` | resolves `undefined`; `t` never called |
| GS13 | AE18, page part: a front whose script, only when `typeof pycmd !== "undefined"`, hides `#qa` and calls `pycmd("ans")` | Mac: an `ans` `command` signal; iPhone: no exception, `#qa` shown with the question |
| GS14 | `document.title` | empty string |
| GS15 | S15, the page side of AnkiMobile's bridge: on iPhone, with the `cb` handler registered as U67's configuration does, `typeof webkit.messageHandlers.cb.postMessage`, then `postMessage(JSON.stringify({scheme: "ankitap", msg: "midCenter"}))`; on Mac, `typeof webkit`; on both, `typeof AnkiDroidJS` | `"function"`, and the call returns without throwing (what native does with it is U67's test); `"undefined"`; `"undefined"` |

### C16.4 BaseCSSTests (R479–R487)

Computed values, light appearance unless stated:

| # | Element or query | Expected |
|---|---|---|
| BC1 | `body` margin | 20px on all sides |
| BC2 | a `div` in `#qa` box-sizing; `#typeans` box-sizing and width | `content-box`; `border-box`, full width |
| BC3 | `.typeGood`, `.typeBad`, `.typeMissed` backgrounds and colours | `rgb(170, 255, 170)`, `rgb(255, 170, 170)`, `rgb(204, 204, 204)`; all `rgb(0, 0, 0)` |
| BC4 | `#_flag` | `position: fixed`, `right: 10px`, `top: 0px`, `font-size: 30px` |
| BC5 | `#_mark` | `left: 10px`, colour `rgb(255, 255, 0)` |
| BC6 | template `#_flag { right: 50px }`; template `[dir] #_flag { right: 50px }` | 10px; 50px |
| BC7 | every C4.1 and C4.2 token on `:root`, light and dark | the table text |
| BC8 | `--focus-color` on `<body>` on Mac, on `<html>` | `rgba(0 103 244 / 0.247)`; `#6366f1` |
| BC8b | `--focus-color` (C4.2). iPhone: a side `<div class="isMac"><span>x</span></div><p>y</p>`, read on the `div`, the `span` and the `p`; then `<html>` given class `isMac` by script, read on `<html>` and the `p`. Mac, body class `card card1 isMac`: a card rule `{ --focus-color: red }` with selector `.isMac`, `body.isMac`, `html body.isMac` (below 0,2,0), then `.card.isMac`, `html .card.isMac` (0,2,0 and 0,2,1), read on `<body>` | `rgba(0 103 244 / 0.247)` twice, `#6366f1`; `#6366f1` twice. `rgba(0 103 244 / 0.247)` for the first three selectors, `red` for the last two |
| BC9 | `html` `-webkit-text-size-adjust`; `html` font-family | `100%`; begins with `system-ui` |
| BC10 | `html` and `body` font-size; an element at `1rem` | 16px |
| BC11 | `hr` | height 1px, background `rgb(115, 115, 115)` |
| BC12 | `img` | max-height `95vh` |
| BC13 | `button` | margin `16px 8px` at 16 px font; `appearance` is not `none` |
| BC14 | `.replay-button svg` | 40px × 40px |
| BC15 | `#image-occlusion-container` and its `img` | C4.4 values |

### C16.5 NightModeTests (S13; R488, R489, R491, R492)

The side CSS is `.card { background: white; color: black }`. NM2 adds `.card.nightMode { background: navy }`.

| # | Setup | Expected |
|---|---|---|
| NM1 | dark | `html` classes start with `night-mode`, `data-bs-theme="dark"`; body classes exactly C3.1; body background `rgb(44, 44, 44)`, colour `rgb(252, 252, 252)`; `matchMedia("(prefers-color-scheme: dark)").matches` |
| NM2 | dark, with the added rule | body background navy |
| NM3 | light | none of the night classes; body background white, colour black (the card's rule); prefers-color-scheme light |
| NM4 | dark, `<img class="latex">`, `<img class="drawing">`, plain `<img>`, `<span class="latex">` | filters `invert(100%)`, `invert(1) hue-rotate(180deg)`, none, `invert(100%)` |
| NM5 | dark and light, `html` and `body` | no `filter` |
| NM6 | `#image-occlusion-canvas` with card-defined colour custom properties | the same values in both appearances |
| NM7 | body and html class strings for each platform in both appearances | C2.2 and C3.1 exactly (D19, D20) |

### C16.6 TypeInRuntimeTests (R508, R534–R536; AE17 IME part)

Each case dispatches synthetic events on an element of the question side. The question has no `{{type:}}` unless stated.

| # | Input | Observed |
|---|---|---|
| TI1 | `keydown`/`keypress` Enter whose handler calls `_typeAnsPress()` | one reveal `command` |
| TI2 to TI6 | the C10.4 table rows 2 to 6 | as the table says |
| TI7 | `keypress` with key `a` | no reveal |
| TI8 | `_typeAnsPress()` called outside any event | throws `TypeError` |
| TI9 | the same Enter on the answer side | no reveal |
| TI10 | question with `<input id=typeans>`, unpresented then presented | `document.activeElement` is not `#typeans` before presentation and is after |
| TI11 | type `abc` in `#typeans`; `getTypedAnswer()`; then show the answer | `"abc"`, and still `"abc"` after the answer replaced the input; `null` on a page whose questions never had `#typeans` |
| TI12 | question with `<div id=typeans>x</div>`, presented | `getTypedAnswer()` is `null` |

### C16.7 AE15 (R467, R468, R521, R522)

A synthetic note type in Migaku's style. Its back side has three scripts, in this order:

1. throws a `TypeError` on its first line (`null.x`);
2. defines a function that, for every `.word` element, appends an `<svg class="pitch">` drawn from the element's `data-pitch` attribute. It runs that function at once when `document.readyState` is `complete`, otherwise on `DOMContentLoaded`, and also makes it callable by script 3;
3. turns the raw syntax `漢字[かんじ;h]` in a container into `<span class="word" data-pitch="h"><ruby>漢字<rt>かんじ</rt></ruby></span>` and calls script 2's function.

Observed, on the first card in a page and again on the next card in the same page:

- scripts 2 and 3 ran;
- `.word` spans and their `svg.pitch` exist;
- the side's text contains no `[`, `;h]` or `かんじ;` from the raw syntax;
- one `exception` diagnostic with phase `script`, index 1, line 1, side `answer`;
- script 2's `readyState` check took the `complete` branch both times (C2.4).

## C17. Oracle, scenarios and goldens

Anki's reviewer page (its HTML, CSS and JavaScript, and the GUI code that drives it) ships in Anki's desktop GUI package. The oracle runs only the `anki` package, which has no web view and none of these files. So:

- there are no conformance scenarios or goldens for this spec, and no oracle work is needed;
- the C16 cases, with expected values taken from this spec, are the verification;
- the hand-captured reference screenshots of R1913 (U240+) cover the visual side later.

## C18. Implementation constraints

These are the traps the implementer writes into `CardRuntime/AGENTS.md`:

- **No extra page globals.** Page-world code is wrapped in one called function expression. It defines globals only by the C9.1 assignments, never by top-level `var`, `function`, `let`, `const` or `class`. Any helper a card could see is a C9 violation, even under a "private-looking" name.
- **Classic-script semantics for cards.** Never change a card script's `type`, never add `"use strict"` or a wrapper to card code, never snapshot the script list before the walk (C6.5), and never re-run scripts by `cloneNode`.
- **No Anki assets** (R1960). The runtime, base stylesheet and tests are written from this spec. Only upstream jQuery 3.7.1 is vendored, unmodified, with its notice.
- **Run first.** The runtime, its capture-phase IME tracking (C10.4) and its listener hooks (C13) are in place before any other page script, `reviewExtra` included.
- **Page-world handlers.** Register no message handler in the page world except iOS `cb` (C9.3). Internal traffic goes through the isolated world (C14).
- **Token values are exact text** (C4.1, C4.2). Do not normalize colours.
- **Similarity scan.** The scan's `text` channel compares runs of 8 CSS words with Anki's stylesheet sources, and the values here are fixed. The order of rows in the C4 tables means nothing. Order the declarations inside each rule and group the rules your own way, and reorder again if the scan flags a region. An allowlist entry needs Antoine's approval (`docs/process/provenance.md`).

## C19. Open points

Defaults this spec takes until the owner decides otherwise, and points the owner has settled:

1. **`pycmd` callback value: settled, `null`** (Antoine, 2026-10-17). R507 and the U66 test list say the callback gets `undefined`. Antoine chose `undefined` unless the pinned 26.09.3 tag shows that Anki passes `null`. It does. Desktop's bridge sends the command handler's return value back to the page as JSON and parses it before it calls the callback. The reviewer's handler returns no value for any command. The bridge's own paths return none either: `domDone`, `close`, and a command that arrives after the web view or the collection is gone. So the callback always receives `null` (JSON null). The only exception is an add-on that answers the command itself, and Meguri has no add-ons. C10.5 and case GS5 keep `null`, and R507's `undefined` does not apply. The source references are in the spec-side provenance under `oracle/`.
2. **Root font size.** R486 keeps 16 px "so rem-sized templates render as in Anki", but Anki desktop's reviewer sets the root to 15 px (C15 D2). This spec follows R486's explicit 16 px. If Antoine wants desktop parity, C4.3 adds `html { font-size: var(--font-size) }` and D2 goes away.
3. **No `window.webkit` in the Mac page world** (C9.3). U67 plans one message handler per world. Under this spec, Meguri's handler is registered only in isolated worlds, plus `cb` in the iOS page world, and page-world traffic goes through the isolated world (C14). U65's S1 record agrees: its confirmation handler is added for Meguri's isolated world only, and the page world cannot reach it.
4. **Things a hidden page still does** (C8.2, C15 D7): Web Audio, speech synthesis, timers and dialogs (`alert`, `confirm`, `prompt`, U70) started by a pre-rendered question happen before the card is shown. A dialog requested by an unpresented page must not block its settling, or the prepare would never settle. U70 decides how such dialogs are deferred or refused. U175 may decide not to pre-render such cards.
5. **R534 keyboard on iOS.** Settled on the simulator by U65's S1(e) (C6.4); the device check is a U256 checkpoint. If the device check fails, the fallback S1(e) names (keyboard on first tap) and its deviation replace the C6.4 sentence.
6. **U65 records not yet integrated.** C2.1, C6.4 and C6.6 cite U65's S1 and S2 records in `docs/design/decisions/`, which are on `unit/U65` while its review is open (`docs/process/review-log/U65.md`). The open findings there concern the Mac frame gate, iOS audio seeking and WebContent memory, not the origin form, the settled definition or S1(e). If U65's integration changes any of these three, this spec follows.

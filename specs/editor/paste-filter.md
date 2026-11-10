# Paste filters

Spec: editor/paste-filter
Requirements: R872, R873, R874, R875, R877
Anki: 26.09.3
Scenarios: conformance/scenarios/editor/paste/

The three filters pasted and dropped HTML goes through before it reaches a field: strip mode, keep mode and internal mode. `editor/paste` P14 and P15 state the rules Meguri implements (U197's page filter); this file gives Anki 26.09.3's behavior for each of them in full, as tables of elements, attributes and style properties and as input-output examples, so that the filter and its conformance tests (`PasteFilterConformanceTests`) can be built without Anki. Where Meguri's column follows a rule of `editor/paste`, that rule decides; a row that contradicts it is a defect of this file. Which content takes which filter is `editor/paste` P9 and `editor/paste-sources` S3; both Anki editors (`editor/paste-sources` S1) run the same filters, and X3 lists where their pre-pass differs.

## X1. Terms

- **External HTML**: HTML from the pasteboard or a drop (`editor/paste` P9 step 2), filtered by X4 in strip mode or X5 in keep mode.
- **Internal content**: content copied from a field, dropped from another field, or made by the URL, image, audio and text steps (`editor/paste` P9), filtered by X7.
- **Kept**: the element stays with the attributes its row names and no others. **Unwrapped**: the element is replaced by its child nodes. **Removed**: the element goes with everything inside it.
- **Empty element**: an element whose serialized content is the empty string after its children were filtered (no child nodes left). An element holding only whitespace text is not empty.

## X2. Parsing and serialization

- Meguri (`editor/paste` P14): the HTML is parsed as an HTML fragment into an inert document (no scripts run, nothing loads), the filter changes that tree, and the tree is serialized once. P25's two page calls parse the same text with the same parser, so they see the same tree.
- Anki parses more than once: the page editor parses external HTML for its image pre-pass and serializes it, then parses the result again in the filter, and the `p` to `div` step re-parses each `p`'s already filtered content into a new `div` of the live document (so an `img` inside a `p` starts loading there). The re-parse can normalize the content again. Any difference this makes is covered by `editor-paste-html-parser`.
- Fragment parsing drops a doctype and the `html`, `head` and `body` tags but keeps their contents; elements that belong in a document head (`title`, `meta`, `link`, `style`, `script`, `base`) become ordinary elements of the fragment, so the filters meet them. CR and CR LF become LF.
- Serialization writes text with `&`, `<`, `>` and U+00A0 as `&amp;`, `&lt;`, `&gt;` and `&nbsp;`, attribute values in double quotes with `&`, `"` and U+00A0 escaped, and attribute and tag names in lower case. Engines differ in a few details (recent Chromium versions also escape `<` and `>` in attribute values); the harness compares as `editor/paste` P15.4 says.
- The filter visits the tree depth first and filters an element's children before the element itself. Comments are removed in every mode, internal included. Text is never changed by a filter; only the output step (X6) touches whitespace.
- Tag names are matched for HTML elements only; elements of SVG and MathML (`svg`, `circle`, `math`, `mi`) are never on a list. Attribute names match ASCII case-insensitively, and names with a prefix (`xlink:href`, `o:spt`) match no row.

## X3. Pre-pass (P14)

Before the filter, external HTML goes through a pre-pass that fetches remote images and stores data images (`editor/paste` P14.2, `editor/paste-sources` S5, S6). The two Anki editors differ in what else it does:

| | Host editor | Page editor | Meguri |
|---|---|---|---|
| parser | a lenient parser that is not HTML5, whose output is serialized again and parsed again by the page | the page's HTML5 fragment parser only | one HTML5 parse (P14, `editor-paste-html-parser`) |
| `script`, `iframe`, `object`, `style` | removed with their content before images are fetched | left for the filter, which removes them (X4) | removed before images are fetched (P14.1) |
| `p` | renamed to `div`, attributes kept for the filter to remove | left for the filter, which replaces it with a new `div` | becomes a `div` (P14.1) |
| internal content | none of the above; only the media-server rewrite of `editor/paste-sources` S11 | the same | none (`editor/paste-sources` S11) |

Images inside `script`, `iframe`, `object` or `style` are never fetched by Meguri or the host editor. The content of `script`, `style` and `iframe` is text to an HTML parser, so it holds no `img` element; the content of `object` is parsed as markup, so the page editor fetches and stores an `img` inside an `object` before its filter removes the `object` (the file stays unused in the media folder; proposed deviation `editor-paste-object-image`, page editor). Unclosed elements (`<p>a<p>b`) can come out nested from the host editor's first parse; the golden records the field.

## X4. Strip mode (R873)

| Element | Result |
|---|---|
| `br`, `div`, `sub`, `sup` | kept, no attributes |
| `img` | kept, `src` and `alt` only |
| `p` | a `div` with no attributes holding the same children |
| `title`, `script`, `iframe`, `object`, `style` | removed |
| any other element that is not empty (X1) | unwrapped |
| any other element that is empty | removed |

So `a`, `b`, `i`, `u`, `em`, `strong`, `span`, `font`, lists, tables, headings, `pre`, `code`, `ruby` and every unknown element are unwrapped when they hold something and removed when they hold nothing. `meta`, `link` and `base` are always empty and so always removed; `template` is unwrapped into nothing, since its contents are not its child nodes. A kept `div`, `sub`, `sup` or `br` stays even when it is empty: `<p></p>` gives `<div></div>`.

## X5. Keep mode (R874, R875)

Every row of X4, and these elements kept with the attributes named:

| Element | Attributes kept |
|---|---|
| `b`, `blockquote`, `code`, `dd`, `dl`, `dt`, `em`, `h1`, `h2`, `h3`, `i`, `li`, `ol`, `pre`, `rp`, `rt`, `ruby`, `strong`, `table`, `u`, `ul` | none |
| `a` | `href` |
| `font` | `color` (Anki in both appearances; Meguri only in light appearance, `editor/paste` P15.2) |
| `td`, `th` | `colspan`, `rowspan` |
| `tr` | `rowspan` |
| `span` | `style`, filtered by X5.1 |

- These elements stay even when empty: `<b></b>` and `<span></span>` stay.
- Not listed, so unwrapped or removed as in X4: `h4`, `h5`, `h6`, `tbody`, `thead`, `tfoot`, `caption`, `col`, `colgroup`, `section`, `article`, `header`, `footer`, `nav`, `main`, `figure`, `figcaption`, `hr`, `s`, `strike`, `del`, `ins`, `mark`, `small`, `big`, `q`, `cite`, `abbr`, `kbd`, `samp`, `var`, `label`, `input`, `button`, `select`, `textarea`, `form`, `video`, `audio`, `source`, `picture`, `canvas`, `noscript`, and unknown or prefixed elements (`o:p`, `w:sdt`).
- An unwrapped `tbody` leaves its rows directly in the `table`; HTML insertion (X8) may add a `tbody` back.
- Meguri only: an `href` that starts with `javascript:` after leading ASCII whitespace and controls (any case) is removed (`editor/paste` P15.2, `editor-paste-script-href`).

### X5.1 `span` styles

Every attribute of a `span` but `style` is removed. Within `style`, the declarations are read as the CSS object model holds them: shorthands expanded into their longhands, declarations the parser does not accept (unknown properties such as `mso-spacerun`, invalid values) not listed at all. Each listed property is kept only if its row allows it:

| Property | Light appearance | Dark appearance (R875) |
|---|---|---|
| `color` | kept | removed |
| `background-color` | kept unless its serialized value is `transparent` (any letter case in the source, `!important` ignored); `rgba(0, 0, 0, 0)` is kept | removed |
| `font-weight`, `font-style`, `text-decoration-line` | kept | kept |
| any other | removed | removed |

- **Rewrite only on removal.** The `style` attribute is written again (P15.4's serialization) only when at least one property was removed. When none was, it stays exactly as the source wrote it, spacing and declarations the parser dropped included: `<span style="color:red">` stays `<span style="color:red">` in light appearance, and Word's `<span style="mso-spacerun:yes">` stays as written in both appearances.
- A `style` from which every property was removed becomes `style=""`.
- The tests of this table read the CSS object model: a property's value is its serialized value, never the source text.
- Shorthands keep only their allowed longhands, in the order the shorthand expands to: `font: italic bold 12px Arial` leaves `font-style: italic; font-weight: bold;`; `text-decoration: underline red` leaves `text-decoration-line: underline;`; `background: yellow` leaves `background-color: yellow;` (light). X9 gives the expected values; where a golden of the editor-page oracle differs from them in order or spelling, the golden decides and this file is corrected.
- `!important` stays with a kept property.
- Dark appearance is Anki's night mode at the moment of the paste; for Meguri the editor's effective appearance (`editor/paste` P15.2).

## X6. Output step

After serialization:

| Mode | Step |
|---|---|
| strip | every run of LF, TAB and space (U+000A, U+0009, U+0020 only) becomes one space, in text and attribute values alike; then the result is trimmed (`editor/paste` P2) |
| keep | trimmed |
| internal | trimmed |

Since U+00A0 is serialized as `&nbsp;` (X2), no-break spaces survive both the collapse and the trim. Collapsing inside attribute values changes names: strip mode turns `<img src="my  pic.png">` into `<img src="my pic.png">`. For Meguri, P14.2's `src` rewrites carry P16's src encoding before the filter runs (`editor/paste` P25), so a name Meguri wrote holds no space, TAB or LF and the collapse never changes it: a fetch stored as `a  b.png` is `src="a%20%20b.png"`. An empty result inserts nothing (`editor/paste` P16).

## X7. Internal mode (R872, R877)

- Every element and attribute is kept, event handler attributes included (internal content is the editor's own).
- For each HTML element with a `style`, these properties are removed: `font-size`, `font-family`, `width`, `height`, `max-width`, `max-height`, and `vertical-align` on `img` elements only. Every other property stays, `text-align`, `color`, `float` and `line-height` included. SVG and MathML elements keep their `style`.
- `style` is rewritten only when a property was removed, as in X5.1; one left with no property is `style=""`.
- Comments are removed; the result is trimmed (X6).

## X8. Insertion

Anki inserts the filtered HTML with the browser's insert-HTML editing command, which parses it again in the field's context and can change it further: it can put the surrounding text's inline style on inserted text, add a `tbody` to a table, merge or split blocks, and drop or add a trailing `<br>`. Meguri inserts the parsed nodes itself (`editor/paste` P16). Goldens recorded from Anki's editor include the command's changes; where Meguri's field differs only by them, the difference is proposed deviation `editor-paste-insert-command`, with each case listed beside its golden.

## X9. Examples

External HTML in each mode (light appearance unless the column says dark), and internal content. "Meguri" in a cell marks where Meguri differs from Anki; P15.4 decides style spelling, so cells write styles as the CSS object model serializes them. A row marked † shows X3's pre-pass and `editor/paste` P14.2 as well, with a fetch that succeeds. These values were worked out from the rules; until the editor-page oracle records goldens they are the expected values of `PasteFilterConformanceTests` (X10).

| Input | Strip | Keep, light | Keep, dark |
|---|---|---|---|
| `<b>heavy</b> <i>slant</i>` | `heavy slant` | `<b>heavy</b> <i>slant</i>` | the same as light |
| `<p class="MsoNormal">a<o:p></o:p></p>` | `<div>a</div>` | `<div>a</div>` | `<div>a</div>` |
| `<p class="x">a</p><p></p>` | `<div>a</div><div></div>` | the same | the same |
| `<a href="https://ex.com" target="_blank">x</a>` | `x` | `<a href="https://ex.com">x</a>` | the same as light |
| `<a name="n"></a>x` | `x` | `<a></a>x` | the same as light |
| `<a href="JavaScript:void(0)">j</a>` | `j` | Anki `<a href="JavaScript:void(0)">j</a>`; Meguri `<a>j</a>` | the same as light |
| `<span style="color: red; font-size: 20px">r</span>` | `r` | `<span style="color: red;">r</span>` | `<span style="">r</span>` |
| `<span style="color:red">r</span>` | `r` | `<span style="color:red">r</span>` | `<span style="">r</span>` |
| `<span style="font-weight:700">x</span>` | `x` | `<span style="font-weight:700">x</span>` | the same as light |
| `<span style="background-color: transparent; font-weight: 700">t</span>` | `t` | `<span style="font-weight: 700;">t</span>` | the same as light |
| `<span style="color: #000; background-color: yellow; font-style: italic">x</span>` | `x` | `<span style="color: #000; background-color: yellow; font-style: italic">x</span>` (nothing removed, so as written) | `<span style="font-style: italic;">x</span>` |
| `<span style="mso-spacerun:yes">  </span>x` | `x` | `<span style="mso-spacerun:yes">  </span>x` | the same as light |
| `<span style="font: italic bold 12px Arial">x</span>` | `x` | `<span style="font-style: italic; font-weight: bold;">x</span>` | the same as light |
| `<span style="text-decoration: underline red">u</span>` | `u` | `<span style="text-decoration-line: underline;">u</span>` | the same as light |
| `<span style="background: yellow">y</span>` | `y` | `<span style="background-color: yellow;">y</span>` | `<span style="">y</span>` |
| `<span style="background-color: TRANSPARENT; color: red">t</span>` | `t` | `<span style="color: red;">t</span>` | `<span style="">t</span>` |
| `<span style="background-color: rgba(0,0,0,0)">z</span>` | `z` | `<span style="background-color: rgba(0,0,0,0)">z</span>` (nothing removed, so as written) | `<span style="">z</span>` |
| `<span class="c" id="d" onclick="f()">t</span>` | `t` | `<span>t</span>` | the same as light |
| `<span></span>x<b></b>` | `x` | `<span></span>x<b></b>` | the same as light |
| `<font color="#ff0000" face="Arial" size="4">f</font>` | `f` | `<font color="#ff0000">f</font>` | Anki `<font color="#ff0000">f</font>`; Meguri `<font>f</font>` |
| `<b style="font-weight:normal;" id="docs-internal-guid-1"><span style="font-weight:700">x</span></b>` | `x` | `<b><span style="font-weight:700">x</span></b>` | the same as light |
| `<div onmouseover="track()" style="color:red">note</div>` | `<div>note</div>` | `<div>note</div>` | the same |
| `<img src="fig.png" alt="Figure" width="100" onload="track()" style="float:left">` | `<img src="fig.png" alt="Figure">` | the same | the same |
| `<img src="my  pic.png">` | `<img src="my pic.png">` | `<img src="my  pic.png">` | the same as light |
| † `<p>see <img alt="d" src="data:image/png;base64,aW5saW5lIGdpZiBieXRlcw=="></p>` | `<div>see <img alt="d" src="paste-bde96ed1fbd958a421a14ce5cedd73191801c63d.png"></div>` | the same | the same |
| † `<img src="https://ex.com/a%20%20b.png">`, stored as `a  b.png` | Anki `<img src="a b.png">` (written raw, then collapsed: another name); Meguri `<img src="a%20%20b.png">` | Anki `<img src="a  b.png">`; Meguri `<img src="a%20%20b.png">` | the same as light |
| `<br class="x"><sub>2</sub><sup>3</sup>` | `<br><sub>2</sub><sup>3</sup>` | the same | the same |
| `<h1>T</h1><h4>U</h4>` | `TU` | `<h1>T</h1>U` | the same as light |
| `<ul><li>a</li><li>b</li></ul>` | `ab` | `<ul><li>a</li><li>b</li></ul>` | the same as light |
| `<table border="1"><tbody><tr class="r" rowspan="2"><td colspan="2" style="color:red">c</td></tr></tbody></table>` | `c` | `<table><tr rowspan="2"><td colspan="2">c</td></tr></table>` (before X8) | the same as light |
| `<pre>a` LF `  b</pre>` | `a b` | `<pre>a` LF `  b</pre>` | the same as light |
| `<div>a` LF TAB `b</div>` LF | `<div>a b</div>` | `<div>a` LF TAB `b</div>` | the same as light |
| LF `  <b>x</b>  ` LF | `x` | `<b>x</b>` | the same as light |
| `<div>  a  </div>` | `<div> a </div>` | `<div>  a  </div>` | the same as light |
| `a&nbsp;&nbsp;b` | `a&nbsp;&nbsp;b` | the same | the same |
| `x<title>Notes</title><script>var n = 2;</script>y<style>p{}</style>` | `xy` | `xy` | `xy` |
| `<!-- c -->a<!--[if gte mso 9]><xml></xml><![endif]-->` | `a` | `a` | `a` |
| `<meta charset="utf-8"><b>x</b>` | `x` | `<b>x</b>` | the same as light |
| `<html><body><!--StartFragment--><b>x</b><!--EndFragment--></body></html>` | `x` | `<b>x</b>` | the same as light |
| `<w-box>c<w-box></w-box></w-box><i>e</i>` | `ce` | `c<i>e</i>` | the same as light |
| `<svg width="10"><circle r="1"></circle></svg>t` | `t` | `t` | `t` |
| `<svg><text>s</text></svg>` | `s` | `s` | `s` |
| `<math><mi>x</mi></math>` | `x` | `x` | `x` |
| `<template>x</template>y` | `y` | `y` | `y` |
| `<DIV CLASS="x">d</DIV>` | `<div>d</div>` | the same | the same |
| `<ruby>漢<rp>(</rp><rt>kan</rt><rp>)</rp></ruby>` | `漢(kan)` | `<ruby>漢<rp>(</rp><rt>kan</rt><rp>)</rp></ruby>` | the same as light |
| `<blockquote cite="u"><code>c</code></blockquote>` | `c` | `<blockquote><code>c</code></blockquote>` | the same as light |
| `<style>p{}</style>` | empty: nothing is inserted | the same | the same |

Internal content (X7):

| Input | Result |
|---|---|
| `<span style="font-size:30px;color:red">x</span>` | `<span style="color: red;">x</span>` |
| `<span style="color:red">x</span>` | `<span style="color:red">x</span>` |
| `<img src="fig.png" style="vertical-align: middle; width: 50px">` | `<img src="fig.png" style="">` |
| `<img src="fig.png" width="50" style="float: left; height: 20px">` | `<img src="fig.png" width="50" style="float: left;">` |
| `<span style="vertical-align: super">x</span>` | `<span style="vertical-align: super">x</span>` |
| `<div style="text-align: center; font-family: Arial">c</div>` | `<div style="text-align: center;">c</div>` |
| `<b class="k" onclick="f()">x</b>` | `<b class="k" onclick="f()">x</b>` |
| `<i>x</i><script>var n = 2;</script>` | `<i>x</i><script>var n = 2;</script>` (kept: internal content is the editor's own, `editor/field-html` F6) |
| `<svg style="width:10px"><rect></rect></svg>` | `<svg style="width:10px"><rect></rect></svg>` |
| `a<!-- c -->b` | `ab` |
| LF ` <b>x</b> ` LF | `<b>x</b>` |

## X10. Oracle and tests

None of this file can be observed by the conformance oracle, which runs Anki's collection library and not its editor page (`editor/paste` P1). Meguri's `PasteFilterConformanceTests` run X9's tables, in WebKit on macOS and in the iOS simulator, through the same page filter that pastes use, comparing styles as P15.4 says. The editor-page oracle records `editor/paste` P24's `strip-filter`, `keep-filter`, `keep-filter-dark`, `internal-paste` and `corpus` goldens from both Anki editors; until it exists these are not goldens, and a mismatch between X9 and such a golden is fixed in this file. Blocked until then: every filter golden, and the plan's corpus tests (Word and Google Docs in keep and strip mode "equals its golden"), which also wait for the M2 capture of `conformance/corpora/paste/`. Until both exist, X9 and `editor/paste-sources` S10 are the interim expected values, and the corpus tests are recorded as blocked, not passed.

# Description HTML

Spec: text/description-html
Requirements: R82
Anki: none (Meguri-defined; no oracle parity)
Scenarios: none (implementer unit tests from the tables below)

Turns a deck description into HTML that is safe to show on the deck page and the finished state (U155), and sanitizes other short HTML Meguri displays from outside sources, such as sync server messages (U161). This is Meguri's own design. Deviation from Anki: Anki shows descriptions without Markdown as raw HTML; Meguri always sanitizes.

## D1. Inputs

- The description text is the deck's `desc` value; the Markdown flag is the deck's `md` value (false when absent). Both come from the deck record unchanged.
- An empty description gives empty output.

## D2. Pipeline

- `md` true: render the text as CommonMark (spec 0.31.2, no extensions: no tables, strikethrough, task lists, footnotes or bare-URL autolinks), with raw HTML blocks and inline HTML passed through; then sanitize the result (D3). A soft line break renders as a line break character, not `<br>`.
- `md` false: sanitize the text as HTML (D3). Markdown syntax stays literal (`**bold**` stays `**bold**`).

## D3. Sanitizer

The input is parsed as an HTML5 fragment (a real parser is right here: the output is only displayed, never stored or compared) and serialized back with only what this section allows. Text is escaped as needed on output.

### D3.1 Elements removed with their content

`script`, `style`, `template`, `iframe`, `frame`, `frameset`, `object`, `embed`, `applet`, `noscript`, `noembed`, `noframes`, `title`, `head`, `link`, `meta`, `base`, `svg`, `math`, `audio`, `video`, `source`, `track`, `canvas`, `map`, `area`, `input`, `button`, `select`, `option`, `optgroup`, `datalist`, `textarea`, `output`, `progress`, `meter`, `dialog`.

### D3.2 Allowed elements

`a`, `abbr`, `b`, `bdi`, `bdo`, `blockquote`, `br`, `caption`, `center`, `cite`, `code`, `col`, `colgroup`, `dd`, `del`, `details`, `dfn`, `div`, `dl`, `dt`, `em`, `figcaption`, `figure`, `font`, `h1`–`h6`, `hr`, `i`, `img`, `ins`, `kbd`, `li`, `mark`, `ol`, `p`, `pre`, `q`, `rb`, `rp`, `rt`, `rtc`, `ruby`, `s`, `samp`, `small`, `span`, `strike`, `strong`, `sub`, `summary`, `sup`, `table`, `tbody`, `td`, `tfoot`, `th`, `thead`, `time`, `tr`, `tt`, `u`, `ul`, `var`, `wbr`.

Any other element (`form`, `fieldset`, `label`, `legend`, `html`, `body`, custom elements, ...) is unwrapped: the tag goes and its children stay, sanitized by the same rules. Comments, doctypes and processing instructions are removed.

### D3.3 Allowed attributes

Every attribute not listed is removed, in particular every event handler (`on*`), `id`, `name`, `class`, `form*`, `srcset`, `data-*`, `background`, `ping` and `target`.

| Element | Attributes |
|---|---|
| any allowed element | `title`, `lang`, `dir`, `style` (D3.5) |
| `a` | `href` (D3.4) |
| `img` | `src` (D3.4), `alt`, `width`, `height` |
| `font` | `color`, `size`, `face` |
| `ol` | `start`, `reversed`, `type` |
| `li` | `value` |
| `td`, `th` | `colspan`, `rowspan`, `align`, `valign` |
| `th` | `scope` |
| `col`, `colgroup` | `span` |
| `p`, `div`, `h1`–`h6` | `align` |
| `time` | `datetime` |
| `details` | `open` |

Every `a` element in the output carries `rel="noopener noreferrer"`. The host opens links in the system browser (U155).

### D3.4 URLs

- `a href` is kept when it is an absolute URL whose scheme is `http`, `https` or `mailto` (ASCII case-insensitive). Any other value (relative, `javascript:`, `data:`, `file:`, other schemes) removes the attribute and keeps the element.
- `img src` is kept when it is an `https` URL, a `data:` URL of type `image/png`, `image/jpeg`, `image/gif`, `image/webp` or `image/avif`, or a bare media file name (no scheme, not starting with `/`, no `..` path segment), which the host resolves in the media folder. Otherwise the whole `img` element is removed. The sanitizer keeps a media file name as written; the page encodes it for display like field HTML (`media-references.md` M6).

### D3.5 Inline style

The `style` value is split at `;`. A declaration `property: value` is kept when the property (trimmed, ASCII case-insensitive) is one of `color`, `background-color`, `font-weight`, `font-style`, `font-size`, `font-family`, `text-decoration`, `text-align` or `vertical-align`, and the value contains none of `url(`, `expression(`, `\`, `<`, `>`, `@`, `/*`, `{` or `}` (ASCII case-insensitive). Kept declarations are joined with `; `; when none is left the attribute is removed.

## D4. Examples

Tests compare structure: element names, attributes and their values, and text, in order. Attribute order, quoting style and a final line break are not part of the contract.

| md | Input | Output |
|---|---|---|
| false | `<script>alert(1)</script><b>x</b>` | `<b>x</b>` |
| false | `<a href="https://example.org" onclick="steal()" target="_blank">l</a>` | `<a href="https://example.org" rel="noopener noreferrer">l</a>` |
| false | `<form action="https://example.org"><input name="q"><b>t</b></form>` | `<b>t</b>` |
| false | `<link rel="stylesheet" href="https://example.org/x.css"><p>p</p>` | `<p>p</p>` |
| false | `<b>bold</b>` | `<b>bold</b>` |
| false | `**bold**` | `**bold**` |
| true | `**bold**` | `<p><strong>bold</strong></p>` |
| true | `[site](javascript:alert(1))` | `<p><a rel="noopener noreferrer">site</a></p>` |
| false | `<img src="https://example.org/a.png" onerror="x()">` | `<img src="https://example.org/a.png">` |
| false | `<img src="http://example.org/a.png">` | (empty) |
| false | `<img src="pic one.jpg">` | `<img src="pic one.jpg">` |
| false | `<span style="color: red; position: fixed">r</span>` | `<span style="color: red">r</span>` |
| false | `<div class="x" id="y">d</div>` | `<div>d</div>` |
| false | `<ruby>漢<rt>かん</rt></ruby>` | `<ruby>漢<rt>かん</rt></ruby>` |

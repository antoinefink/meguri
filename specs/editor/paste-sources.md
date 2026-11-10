# Paste and drop sources

Spec: editor/paste-sources
Requirements: R139, R866, R867, R868, R869, R870, R871, R876, R877
Anki: 26.09.3
Scenarios: conformance/scenarios/editor/paste/

Where pasted and dropped content comes from, and what each source turns into before the filters of `editor/paste-filter` run: the source order, URL items and their downloads, the names of fetched files, image data, audio items, plain text, content copied from a field, drops, and what happens when a step fails. `editor/paste` P8 to P13, P16 and P17 are the rules Meguri implements (U197); this file gives Anki 26.09.3's behavior for every case those rules cover, as input-output tables, so that the implementer can build and test them without Anki. Each table names the `editor/paste` rule it shows. Where Meguri's column follows a rule of `editor/paste`, that rule decides; a row that contradicts it is a defect of this file. Media naming is `media/media-files` M3; the name transform is `text/media-filenames` F1; display encoding and save decoding of names are `text/media-references` M6 and M7.

## S1. Anki's two paste paths

Anki 26.09.3 runs two editors. The **host editor** decides a paste or a drop in the desktop host process; the **page editor** decides it in the editor page. Which one a window gets:

| Window | Flag off (the default) | Collection's experimental-editor flag on |
|---|---|---|
| Add opened from the main window, Edit Current, Browse | host editor | page editor |
| Add opened from Browse | host editor | host editor (the flag is not read) |

Shift held while the window opens inverts that window's choice, in every row.

- `editor/paste` P1 is right: the host editor is the default in every window and the page editor is the experimental one. Both are shipped behavior, so the oracle maintainer records editor goldens from both (S15).
- Both paths share: the two modes and how Shift inverts them (S2, except one detail), the media extension lists (`editor/paste` P2), the plain-text conversion (S10), the three filters (`editor/paste-filter`), the `paste-<sha1>` naming of pasted bitmaps (S8) and the `img` snippet of a name with an image extension.
- Snippets of other names differ (S7): the host editor writes `[sound:NAME]` for every name without an image extension, whatever its extension or none, and plays it. Only the page editor follows `editor/paste` P3's audio-or-video and bare-name rows.
- They differ in the rows of S3 to S13 marked "host" and "page". The summary is S14.

## S2. Mode (P8)

| Situation | Host editor | Page editor | Meguri |
|---|---|---|---|
| plain paste (⌘V, menu Paste) | `pasteStripsFormatting` read at the paste: `false` keep mode, `true` strip mode | the same | P8 |
| Shift held | inverts, read at the moment the host handles the paste or drop | inverts when the drop event itself carries Shift, or when the latest key press in any field of the page had Shift held; that remembered state is one value for the whole page, set by every key press in any of its fields and kept until the next one, so after ⇧⌘V in one field a paste from the context menu, or a drop, into any field is still inverted | P8: the modifiers of that paste or drop only |
| key missing or unreadable | read as `false` | read as `false` | P8, `editor/config-keys` K3 |

The mode decides only what S10 does with text tokens and which filter external HTML goes through. URL, image and audio items are handled the same in both modes (P8).

## S3. Source order (P9)

The pasteboard (clipboard, or drag data for a drop) is tried step by step; the first step that yields markup wins. Steps 3 and 4 yield internal content (filtered by `editor/paste-filter` X7 only), step 2 external HTML (X4 or X5 by mode).

| Step | Host editor | Page editor | Meguri |
|---|---|---|---|
| 1. content copied from a field | S11 | S11 | P9 step 1 |
| 2. HTML | `text/html` present, even empty: an empty value ends the search with nothing pasted | `text/html` with a non-empty value; an empty value counts as absent and the search goes on | P9 step 2 (present HTML ends the search, also when empty) |
| 3, first URL a `file://` URL | URLs, image, text | URLs, image, audio, text | P9 step 3 |
| 3, otherwise | image, URLs, text | image, audio, URLs, text | P9 step 3 |
| "first URL is a `file://` URL" | the toolkit's parsed URL, so the scheme compares case-insensitively | the first line of `text/uri-list`, compared case-sensitively with `file://` | the first URL item's scheme, case-insensitively |
| nothing yields | nothing happens | nothing happens | P9 step 5 |

Pasteboards and the step that wins (`h` HTML, `u` URL list, `i` image, `a` audio file, `t` text):

| Pasteboard | Host editor | Page editor | Meguri |
|---|---|---|---|
| `h` `<b>x</b>` and `t` `x` | HTML | HTML | HTML |
| `h` with an empty value and `t` `hello` | nothing | text `hello` | nothing |
| `h` `<style>p{}</style>` and `t` `hello` | HTML, which the filter empties: nothing | the same | the same |
| `u` `file:///a/b.png` `https://ex.com/c.png`, `i`, `t` | URLs: the file, then the download | the same | the same |
| `u` `https://ex.com/c.png`, `i` | image | image | image |
| `u` `https://ex.com/page`, `t` `https://ex.com/page` | the link (no image) | the same | the same |
| `a` file `clip.mp3`, `t` `clip.mp3` | text `clip.mp3` (no audio step) | the audio file | the audio file (P12) |
| `i`, `t` | image | image | image |
| `t` only | text | text | text |
| `u` whose only line is empty | no URLs; next step | no URLs; next step | no URL items |
| `u` with the lines `https://ex.com/a`, an empty line, `https://ex.com/b` | two links | three items: link, `<a href=""></a>`, link | two links (P10.1 skips empty URLs) |
| `u` holding one item whose text is two lines, `https://ex.com/a` LF `https://ex.com/a` | one link: the first line | the same | the same (P10.1) |

Meguri reads URL items (`public.url`, `public.file-url`, iOS file items), never a `text/uri-list` text, so comment lines and line ends of such a list never reach it.

## S4. URL items (P10.1)

- **Media URL.** A URL is a media URL when its whole text, lowercased, ends with `.` and a media extension, query and fragment included. Both paths and Meguri agree: `https://ex.com/A.MP3` is one, `https://ex.com/a.mp3?x=1` and `https://ex.com/a.png#top` are not.
- A media URL is fetched (S5). On success its file is added and the item becomes the snippet of the name S6 gives; on failure, or for any other URL, it becomes a link.
- **Link.** `<a href="URL">TEXT</a>`:

| | Host editor | Page editor | Meguri |
|---|---|---|---|
| URL as read | the toolkit's display form, in which most `%XX` escapes (spaces, non-ASCII) are already decoded | the line as the list holds it, trimmed | the item's URL text |
| `href` | the URL as read, unescaped | the URL, unescaped | `&` and `"` escaped (P10.1) |
| TEXT | every `%XX` decoded as UTF-8, invalid sequences as U+FFFD, then `&`, `<`, `>`, `"`, `'` escaped | `%XX` decoded except the escapes of `;` `/` `?` `:` `@` `&` `=` `+` `$` `,` `#`, which stay; a malformed escape aborts the whole paste (S13); then the five characters escaped | as the host editor (P10.1) |

| URL | Host editor TEXT | Page editor TEXT | Meguri field |
|---|---|---|---|
| `https://ex.com/a%20b?x=1&y=2` | `https://ex.com/a b?x=1&amp;y=2` | the same | `<a href="https://ex.com/a%20b?x=1&amp;y=2">https://ex.com/a b?x=1&amp;y=2</a>` |
| `https://ex.com/a%2Fb` | `https://ex.com/a/b` | `https://ex.com/a%2Fb` | `<a href="https://ex.com/a%2Fb">https://ex.com/a/b</a>` |
| `https://ex.com/%E6%97%A5` | `https://ex.com/日` | `https://ex.com/日` | `<a href="https://ex.com/%E6%97%A5">https://ex.com/日</a>` |
| `https://ex.com/%E6%97` | `https://ex.com/` U+FFFD | nothing is pasted | `<a href="https://ex.com/%E6%97">https://ex.com/</a>` + U+FFFD |
| `https://ex.com/q?a="b"` | the `"` ends `href` early; the golden records the field | the same | `<a href="https://ex.com/q?a=&quot;b&quot;">https://ex.com/q?a="b"</a>` (`editor-paste-href-escaping`) |

These links come from steps 3 and 4, so they are internal content and survive strip mode (P9).

## S5. Fetching (P10.2)

A fetch serves a media URL (S4), a keep-mode URL token (S10) and every `img` source of external HTML that starts with `http://`, `https://`, `ftp://` or `file://` (`editor/paste` P14.2), whatever its extension. Fetches run one after another, in document or pasteboard order.

| Response or condition | Host editor | Page editor | Meguri |
|---|---|---|---|
| 200 with a body | success | success | success |
| 204, 206 or any other 2xx | failure; the alert gives the status code | success, with whatever body came (possibly empty) | failure (P10.2: final status 200 only) |
| redirects, then 200 | success; up to 30 redirects | success; up to 10 redirects | success; up to 10 redirects, an 11th fails; a redirect to a scheme other than `http` or `https` fails (P10.2) |
| a 3xx that is not followed (304, or a redirect status without `Location`) | failure (200 only) | success, with that response's body; a 304 has no body, so its file is empty | failure (P10.2) |
| 4xx, 5xx | failure | failure | failure |
| no byte for 30 s | failure | failure | failure |
| bytes keep arriving, but the whole exchange takes more than 30 s | success | failure: 30 s is the limit for the whole request, body included | success (P10.2: no data for 30 seconds) |
| refused connection, unknown host, TLS failure | failure | failure | failure |
| `ftp://` | failure | failure | failure (P10.2) |
| `file://` URL of a readable file | its bytes | its bytes | its bytes |
| `file://` URL of a missing or unreadable file | failure | failure | failure |
| `file://` URL whose `%XX` escapes are not UTF-8 | failure (the decoded path names no file) | failure: refused as invalid input | failure |
| `file://` URL with an encoded `#` (`…/a%23b.png`) | reads `a#b.png` | decodes the whole URL first, so `#` starts a fragment and the file read is `a`: failure unless `a` exists, and then the name is still `a#b.png` (S6 works on the URL as given), holding `a`'s bytes | reads `a#b.png` (P10.2) |
| a body sent with `Content-Encoding: gzip` or `deflate` | asks for gzip and deflate and stores the decoded bytes | does not ask for compression and stores the body as received, still encoded | asks for gzip and deflate and stores the decoded bytes (P10.2) |
| User-Agent | `Anki` and the version | `Mozilla/5.0 (compatible; Anki)` | P10.2 |

- **Failure.** Both paths show one alert per failed fetch, then the item takes its fallback: a link (S4, S10) or the `src` left as it was (P14.2). The host editor's alert names the error, or gives the status code when the status was not 200. The page editor's alert is the collection library's localized error text: the network-error summary (offline, timeout, other) with details, and for some statuses the wording that status has in sync (403 the wrong-password message, 407 the proxy-authentication message, 500, 502, 503 and 504 the server-error message, 501 the update-required message, 400 the check-database message, 409 the conflict message). Meguri shows P21's own wording instead (`editor/paste` P21).
- Files a paste added before a failure stay in the media folder (`editor/paste` P4, written at once).

## S6. Names of fetched files (P10.3)

The name a fetch adds its bytes under.

- **Host editor.** The name is the text after the last `/` of the percent-decoded URL without its query (the query starts at the first `?`). So an encoded `/` splits the name, and a fragment stays in it.
- **Page editor.** The name is the percent-decoded last segment of the normalized URL path (dot segments resolved, spaces encoded), query and fragment excluded. So an encoded `/` stays in the name. A segment whose escapes are not valid UTF-8 refuses the fetch.
- **Both.** An empty or whitespace-only name is `paste`. An extension from the media type is appended only when the name has none and the `Content-Type` value equals a row of P10.3's table exactly, parameters and case included. "Has none" differs: for the host editor a name has none when it holds no `.` after its leading dots (`.hidden` and `..png` have none); for the page editor when it holds no `.`, is `..`, or holds `.` only as its first character (`.hidden` has none, `..png` has `png`). Both see an extension in `a.`.

| URL | `Content-Type` | Host editor | Page editor | Meguri (P10.3) | Stored by M3 (for Meguri's name, empty folder) |
|---|---|---|---|---|---|
| `https://ex.com/dir/Pic.PNG?size=2` | `image/png` | `Pic.PNG` | `Pic.PNG` | `Pic.PNG` | `pic.png` |
| `https://ex.com/img?id=3` | `image/png` | `img.png` | `img.png` | `img.png` | `img.png` |
| `https://ex.com/` | `image/jpeg; charset=binary` | `paste` | `paste` | `paste.jpg` (`editor-paste-mime-params`) | `paste.jpg` |
| `https://ex.com/x` | `Image/PNG` | `x` | `x` | `x.png` (`editor-paste-mime-params`) | `x.png` |
| `https://ex.com/dir/` | `image/png` | `paste.png` | `paste.png` | `paste.png` | `paste.png` |
| `https://ex.com/%20` | `image/png` | `paste.png` | `paste.png` | `paste.png` | `paste.png` |
| `https://ex.com/a.png#top` (an `img` source) | `image/png` | `a.png#top` | `a.png` | `a.png` (`editor-paste-url-fragment`) | `a.png` |
| `https://ex.com/dir/a%2Fb.png` | `image/png` | `b.png` | `a/b.png` | `b.png` | `b.png` |
| `https://ex.com/a%20b.png` | `image/png` | `a b.png` | `a b.png` | `a b.png` | `a b.png` |
| `https://ex.com/%E6%97%A5.png` | `image/png` | `日.png` | `日.png` | `日.png` | `日.png` |
| `https://ex.com/%FF.png` | `image/png` | U+FFFD `.png` | refused: the fetch fails (S13) | U+FFFD `.png` | U+FFFD `.png` |
| `https://ex.com/x/../y.png` | `image/png` | `y.png` | `y.png` | `y.png` | `y.png` |
| `https://ex.com/.hidden` | `image/png` | `.hidden.png` | `.hidden.png` | `.hidden.png` | `.hidden.png` |
| `https://ex.com/..png` | `image/png` | `..png.png` | `..png` | `..png.png` | `..png.png` |
| `https://ex.com/a.` | `image/png` | `a.` | `a.` | `a.` | `a._` (F1 step 5) |
| `https://ex.com/play?f=a.mp3` | `audio/webm` | `play.weba` | `play.weba` | `play.weba` | `play.weba` |
| `https://ex.com/play?f=a.mp3` | `application/octet-stream` | `play` | `play` | `play` | `play` |
| `file:///Users/me/b%20c.JPG` | none | `b c.JPG` | `b c.JPG` | `b c.JPG` | `b c.jpg` |

`play?f=a.mp3` is a media URL (S4: its text ends with `.mp3`), so it is fetched although its path has no extension.

After redirects, both paths name the file from the URL they were asked to fetch and take the media type from the final response: `https://ex.com/moved` answered with a redirect to an `image/png` response is named `moved.png`. Recorded for the page editor by `editor/paste/url/status` (S15).

### S6.1 The name the field references

The host editor puts the name the media store returned into the field. The page editor puts the name it asked for, before M3's transform, lowercasing and hash renaming, so its field can name a file that is not there:

| Folder before | Fetched name | Stored (M3) | Host editor's snippet names | Page editor's snippet names | Meguri |
|---|---|---|---|---|---|
| empty | `Pic.PNG` | `pic.png` | `pic.png` | `Pic.PNG` | `pic.png` |
| `pic.png` with other bytes | `pic.png` | `pic-<hash>.png` | `pic-<hash>.png` | `pic.png` (the other file) | `pic-<hash>.png` |
| empty | `a/b.png` (page only) | `ab.png` | not reached | `a/b.png` | not reached (Meguri's name is `b.png`) |

Meguri always uses the stored name (`editor/paste` P10.1, P14.2): proposed deviation `editor-paste-url-stored-name`.

## S7. Snippets and their `src` encoding (P3, P16)

A name with an image extension gets an `img` snippet in both paths. Any other name gets `[sound:NAME]` (played) from the host editor, whatever its extension; the page editor follows `editor/paste` P3, as Meguri does: `[sound:NAME]` for an audio or video extension, the bare name otherwise. The `img` snippet's `src` is encoded differently, and the save then decodes it with `text/media-references` M7 in both paths:

| Stored name | Host editor's snippet | Page editor's snippet | Meguri's snippet (P3) | Field as saved: host editor and Meguri | Field as saved: page editor |
|---|---|---|---|---|---|
| `a&b c.png` | `<img src="a%26b%20c.png">` | `<img src="a&b%20c.png">` | as the host | `<img src="a&b c.png">` | `<img src="a&amp;b c.png">` |
| `日本.png` | `<img src="%E6%97%A5%E6%9C%AC.png">` | the same | the same | `<img src="日本.png">` | the same |
| `100%.jpg` | `<img src="100%25.jpg">` | the same | the same | `<img src="100%.jpg">` | the same |
| `a%41.png` | `<img src="a%2541.png">` | the same | the same | `<img src="a%41.png">` | the same |
| `a#b.png` | `<img src="a%23b.png">` | `<img src="a#b.png">` | as the host | `<img src="a#b.png">` | the same |
| `it's (1).png` | `<img src="it%27s%20%281%29.png">` | `<img src="it's%20(1).png">` | as the host | `<img src="it's (1).png">` | the same |
| `clip&1.mp3` | `[sound:clip&amp;1.mp3]` | the same | the same | `[sound:clip&amp;1.mp3]` | the same |
| `play` | `[sound:play]`, played | `play` | `play` (P3, `editor-paste-bare-name`) | host `[sound:play]`; Meguri `play` | `play` |
| `play.weba` | `[sound:play.weba]`, played | `play.weba` | `play.weba` | host `[sound:play.weba]`; Meguri `play.weba` | `play.weba` |

- The host editor (and Meguri, P3) writes each UTF-8 byte of the name as `%` and two upper-case hex digits except ASCII letters, digits, `_`, `.`, `-`, `~` and `/`. The page editor keeps more characters as they are: ASCII letters, digits and `;` `,` `/` `?` `:` `@` `&` `=` `+` `$` `-` `_` `.` `!` `~` `*` `'` `(` `)` `#`; every other byte is written as `%` and two upper-case hex digits.
- The page editor's `&` reaches the field as a raw `&` in an attribute, which the page writes back as `&amp;` when the field is saved; M7 leaves entities alone. Both stored forms reference `a&b c.png` (`text/media-references` M4 decodes entities).
- A `src` that a fetch rewrote (P14.2) is written raw by both paths, so a stored name holding `%` and two hex digits is decoded at save into another name: a fetch stored as `a%41.png` is saved as `<img src="aA.png">`. Meguri encodes it (P16, `editor-paste-src-encoding`).
- The scenario `editor/paste/store/snippet-save` observes the save's decoding and reference extraction of every snippet in this table (S15).

## S8. Image data (P7, P11)

| | Host editor | Page editor | Meguri |
|---|---|---|---|
| where image data comes from | the system clipboard's image, through the desktop toolkit | the first of these types present as data or as a pasted or dropped file of that type: `image/jpeg`, `image/png`, `image/gif`, `image/svg+xml`, `image/bmp`, `image/x-portable-bitmap`, `image/x-portable-graymap`, `image/x-portable-pixmap`, `image/x-xbitmap`, `image/x-xpixmap` | P9's table |
| encoding | the toolkit's encoder: JPEG at quality 80, or, when `pasteImagesAsPng` is `true`, PNG at the toolkit's quality 50 (which sets the compression level) | the same encoder with the same settings, called from the page | P7 (Apple's encoder) |
| name | `paste-` + SHA-1 of the encoded bytes + `.jpg` or `.png` | the same | the same (P7) |
| a dropped image file of a listed type with no URL list | not reached (a file drop carries its URL) | re-encoded and named as above, so a dropped JPEG file becomes `paste-<sha1>.jpg` | P9: a file drop yields its file URL first |
| data the toolkit cannot decode | an empty file `paste-da39a3ee5e6b4b0d3255bfef95601890afd80709.jpg` (the SHA-1 of no bytes) is added and inserted | the same | decided 2026-11-10: the image step yields nothing and P9 tries the next step; no file is added |

Adding a pasted image twice reuses the file: the second add finds the same name with the same bytes and writes nothing (`media/media-files` M3). `editor/paste/store/image-names` observes these adds (S15).

## S9. Audio items (P12)

The page editor's audio step takes the first pasted or dropped file whose name's extension (after the last `.`, lowercased; the whole name when it has none) is an audio or video extension (`editor/paste` P2), adds its bytes under that name and inserts its `[sound:…]` snippet, playing it once. The host editor has no audio step: such a file reaches it only as a file URL. Meguri: P12.

## S10. Plain text (P13)

The result, in both paths and in Meguri:

- Each LF becomes `<br>`; CR is not a line break.
- `&`, `<`, `>`, `"` and `'` are escaped.
- A TAB counts as four spaces. Within a run of n ≥ 2 spaces the first n − 1 are `&nbsp;` and the last is a plain space; a single space stays a space.
- In keep mode, a whitespace-delimited token that starts with `data:image/` is replaced as P14.2's data rule says, and one that starts with `http://`, `https://`, `ftp://` or `file://` (any case) as S4 says of a URL. In strip mode every token is escaped text.
- The result is internal content, so `editor/paste-filter` X7 only trims it.

| | Host editor | Page editor | Meguri |
|---|---|---|---|
| whitespace that separates tokens | Unicode whitespace including U+001C to U+001F and U+0085, but not U+FEFF | U+0009 to U+000D, U+0020, U+00A0, U+1680, U+2000 to U+200A, U+2028, U+2029, U+202F, U+205F, U+3000 and U+FEFF | P2's text whitespace (the host's set) |
| `data:image/` token of an unsupported type (`webp`), keep mode | the token disappears | the token disappears | escaped text (P14.2, `editor-paste-data-url-kept`) |
| `data:image/png;base64,` token with invalid base64, keep mode | the paste fails (S13) | the paste fails | escaped text |

The token sets differ only where a keep-mode URL or data token meets U+001C to U+001F, U+0085 or U+FEFF: `https://ex.com/p` U+0085 `x` is the link `https://ex.com/p` and the text U+0085 `x` for the host editor and Meguri, and one URL token, a link to the whole text, for the page editor.

Field results (`\t` TAB, `\n` LF, `\r` CR; "either" means both modes):

| Text | Mode | Field gets |
|---|---|---|
| `a\tb   c\nd<e>` | either | `a&nbsp;&nbsp;&nbsp; b&nbsp;&nbsp; c<br>d&lt;e&gt;` |
| `a  b` | either | `a&nbsp; b` |
| `a b` | either | `a b` |
| ` x ` | either | `x` |
| `  x` | either | `&nbsp; x` |
| `x  ` | either | `x&nbsp;` |
| `   ` | either | `&nbsp;&nbsp;` |
| `\t` | either | `&nbsp;&nbsp;&nbsp;` |
| `\t x` | either | `&nbsp;&nbsp;&nbsp;&nbsp; x` |
| `\n` | either | `<br>` |
| `a\n\nb\n` | either | `a<br><br>b<br>` |
| `a\r\nb` | either | `a` LF `<br>b` |
| `say "hi" & 'bye'` | either | `say "hi" &amp; 'bye'` |
| `<b>x</b>` | either | `&lt;b&gt;x&lt;/b&gt;` |
| U+00A0 `x` | either | `&nbsp;x` |
| U+3000 `x` U+3000 | either | `x` |
| U+FEFF `x` U+FEFF | either | `x` |
| `x` U+0085 | either | `x` U+0085 |
| `see https://ex.com/page now` | keep | `see <a href="https://ex.com/page">https://ex.com/page</a> now` |
| `see https://ex.com/page now` | strip | `see https://ex.com/page now` |
| `HTTPS://EX.COM/P` | keep | `<a href="HTTPS://EX.COM/P">HTTPS://EX.COM/P</a>` |
| `(https://ex.com/x)` | keep | `(https://ex.com/x)` (the token does not start with a scheme) |
| `https://ex.com/a.mp3,` | keep | `<a href="https://ex.com/a.mp3,">https://ex.com/a.mp3,</a>` (not a media URL) |
| `https://ex.com/a.mp3` | keep | `[sound:a.mp3]` after a successful fetch; the link after a failed one |
| `https://ex.com/a.mp3?x=1` | keep | `<a href="https://ex.com/a.mp3?x=1">https://ex.com/a.mp3?x=1</a>` |
| `data:image/png;base64,aW5saW5lIGdpZiBieXRlcw==` | keep | `<img src="paste-bde96ed1fbd958a421a14ce5cedd73191801c63d.png">` (the decoded bytes' SHA-1) |
| `data:image/png;base64,aW5saW5lIGdpZiBieXRlcw==` | strip | the token as text |
| `data:image/webp;base64,AAAA` | keep | `data:image/webp;base64,AAAA` (Meguri; Anki drops it) |
| empty | either | nothing is pasted |

## S11. Content copied from a field (P9 step 1)

| | Host editor | Page editor | Meguri |
|---|---|---|---|
| what is remembered | the clipboard's HTML right after a cut or copy in a field | the copy or cut event's own clipboard data, read while the event is dispatched, before the browser writes the selection: expected to be always empty (to be confirmed by the editor-page oracle, `editor/paste` P24 `internal-paste`) | the private type `net.meguri.field-html` (P9 step 1) |
| when it is forgotten | as soon as the clipboard holds anything else | an empty remembered value never takes part, so step 1 never fires | when anything replaces the pasteboard |
| `img` sources `http://127.0.0.1:<port>/<rest>` (the editor's own media server) | rewritten to `<rest>` | the same | not needed: the private type holds storage-form names |
| filter | `editor/paste-filter` X7 | the same | the same |

| Sequence | Host editor | Page editor | Meguri |
|---|---|---|---|
| copy `<b>x</b>` in field 1, paste in field 2 | `<b>x</b>`, internal | the browser's HTML of the selection, external, filtered in the paste's mode (strip mode leaves `x`) | `<b>x</b>`, internal |
| copy in field 1, copy `y` in another app, paste in field 2 | `y` | `y` | `y` |
| copy in field 1, drag rich HTML from a browser onto field 2 | the dragged HTML, external | the same | the same |

The page editor's column follows from the order of events at copy time and is not yet observed; the editor-page oracle confirms it. If it holds, a copy between fields of the page editor is filtered as external content, which Meguri does not reproduce: proposed deviation `editor-paste-field-copy` (page).

## S12. Drop (P17)

| | Host editor | Page editor | Meguri |
|---|---|---|---|
| target | the field under the drop point; outside a field or on a collapsed field nothing is inserted, but the media of a dropped URL, image or text item were already fetched and added; the `img` sources of dropped HTML are not fetched | the rich-text area that receives the drop, focused; a collapsed field shows none, so nothing happens | P17: the target is checked first; nothing is added (`editor-drop-outside-field`) |
| a field showing only its HTML editor | its rich-text view is shown first | the drop lands in the HTML editor, not in this pipeline | P17 |
| caret | the field's restored selection; the golden records where the content lands | the field's selection after the focus | the end of the field (P17) |
| HTML dragged from anywhere inside Anki (a drag whose source is in the app: another field, the same field, another editor window, another Anki web view) | inserted as internal content (`editor/paste-filter` X7 only), whatever the drop's mode | external HTML, filtered in the drop's mode | a drag that started in a Meguri field: internal content (P17, R877) |
| Shift | held at the drop inverts the mode | S2 | held at the drop inverts the mode |
| sources | S3's order, as a paste | S3's order, as a paste | P9 from step 2 |

## S13. Failures

What the field and the media folder hold after each failure (the plan's "recorded failure behavior"):

| Failure | Host editor | Page editor | Meguri |
|---|---|---|---|
| a media URL item or keep-mode URL token cannot be fetched (S5) | alert; the link is inserted | the same | the same (P10.2) |
| an `img` source cannot be fetched, including a server that answers after 31 s | alert; `src` unchanged | alert after at most 30 s; `src` unchanged | alert; `src` unchanged (P10.2, P14.2) |
| the page editor refuses a fetched name that is not UTF-8 (S6) | not reached | alert; the fallback as for a failed fetch | not reached |
| the name transform empties a fetched name (`https://ex.com/%3F%3F` gives `??`) | an error dialog; the paste stops; files added earlier stay | the paste stops with an error; files added earlier stay | P4's refusal alert; that item inserts nothing; the paste goes on |
| a `data:image/{jpg,jpeg,png,gif};base64,` source or token that does not decode | an error dialog; the paste stops | the paste stops | left unchanged (P14.2) |
| base64 with inner line breaks or missing padding | inner whitespace or missing padding fails as above | inner ASCII whitespace ignored; missing padding accepted | inner ASCII whitespace ignored; missing padding does not decode, so unchanged (P14.2) |
| an `img` in external HTML whose source is `data:image/` of another type (`webp`, `bmp`) | `src=""` | `src=""` | left unchanged (P14.2, `editor-paste-data-url-kept`) |
| a malformed `%` escape in a link's URL (S4) | U+FFFD in the link text | the paste stops; nothing is inserted | U+FFFD (P10.1) |
| image data that does not decode (S8) | an empty `paste-<sha1 of nothing>.jpg` is added and inserted | the same | the step yields nothing (S8) |

"The paste stops" means nothing of that paste is inserted; media the paste added before the failure stay, and Check Media later reports them unused (`media/media-files` M13).

## S14. Differences

How each path compares with Meguri's rules. Ids are `conformance/deviations.yaml` entries proposed with the goldens that show them; the ones `editor/paste` P23 already proposes are marked P23. A deviation applies to the goldens of the path whose behavior differs.

| Situation | Host editor | Page editor | Deviation |
|---|---|---|---|
| empty `text/html` (S3) | as Meguri | falls through to later steps | `editor-paste-empty-html` (page) |
| audio step (S3, S9) | absent | as Meguri | `editor-paste-audio-step` (host) |
| empty line in a URL list (S3) | as Meguri | an empty link | `editor-paste-uri-list-empty-line` (page) |
| link text decoding (S4) | as Meguri | reserved escapes kept; malformed escapes abort | `editor-paste-link-text` (page) |
| `"` in a link's `href` (S4) | unescaped | unescaped | `editor-paste-href-escaping` (P23) |
| 2xx other than 200, or a 3xx that is not followed (S5) | as Meguri | success | `editor-paste-fetch-status` (page) |
| compressed body (S5) | as Meguri | stored still encoded | `editor-paste-content-encoding` (page) |
| whole-request 30 s limit (S5) | as Meguri | fails slow downloads | `editor-paste-fetch-timeout` (page) |
| fragment in a fetched name (S6) | kept | as Meguri | `editor-paste-url-fragment` (P23, host) |
| `..png` and names like it (S6) | as Meguri | no extension appended | `editor-paste-leading-dots` (page) |
| `Content-Type` parameters or case (S6) | no extension | no extension | `editor-paste-mime-params` (P23) |
| encoded `/` in a fetched name (S6) | as Meguri | split after decoding | `editor-paste-url-slash` (page) |
| `%XX` escapes that are not UTF-8 in a fetched name (S6) | as Meguri | the fetch fails | `editor-paste-url-invalid-escape` (page) |
| encoded `#` in a `file://` URL (S5) | as Meguri | the fetch reads another path | `editor-paste-file-url-hash` (page) |
| name returned instead of the stored name (S6.1) | as Meguri | the asked-for name | `editor-paste-url-stored-name` (page) |
| `src` encoding of snippets (S7) | as Meguri | a smaller set, so `&` is stored as `&amp;` | `editor-paste-snippet-encoding` (page) |
| snippet of a name with neither an image nor an audio or video extension (S7) | `[sound:NAME]`, played | as Meguri | `editor-paste-bare-name` (P23, host) |
| bitmap bytes (S8) | Qt's encoder | Qt's encoder | `editor-paste-image-bytes` (P23) |
| `data:image/` of another type, or that does not decode (S10, S13) | `src=""`, a text token dropped, bad data stops the paste | the same | `editor-paste-data-url-kept` (P23) |
| fetched `src` with `%` and two hex digits (S7) | decoded at save | decoded at save | `editor-paste-src-encoding` (P23) |
| undecodable image data (S8) | empty file | empty file | `editor-paste-undecodable-image` |
| copy between fields (S11) | as Meguri | filtered as external content (to be confirmed) | `editor-paste-field-copy` (page) |
| drag from another field (S12) | as Meguri | filtered | `editor-drop-field-html` (page) |
| drop outside a field (S12) | media added | as Meguri | `editor-drop-outside-field` (P23, host) |
| Shift remembered from the last key press (S2) | as Meguri | remembered | none: no golden can hold a key state across steps |

## S15. Oracle and scenarios

The conformance oracle runs Anki's collection library, not its editor. Of this file it can observe:

- the media store's handling of every desired name a paste produces: pasted bitmaps and data images (`paste-<sha1>.<ext>`, reuse, the hash suffix), fetched names (S6, S6.1's stored column, `text/media-filenames` F1), through `add_media` and `media_dir` (`specs/conformance/media-ops.md` F5, F9) and the `text` function `media_filename`;
- the save's decoding of every snippet form of S7 and the references the saved field holds, through the `text` functions `iri_decode`, `media_refs` and `iri_encode` (`specs/conformance/observations.md` O9);
- the page editor's fetch: S5's page column, S6's page column and S6.1, through the op `add_media_from_url` against the scenario's own fixture server or file folder (`specs/conformance/editor-ops.md`, S15.1). The page editor makes this fetch through one call of the collection library, so the oracle runs the editor's own fetch, without the editor. These goldens therefore observe the experimental editor's fetch (S1). The default host editor's fetch has no golden: its rules (200 only, 30 redirects, 30 s without data, the fragment kept, decoding before the last `/`) rest on S5's and S6's host column alone.

Scenarios (inactive):

| Scenario | Covers |
|---|---|
| `editor/paste/store/image-names` | S8 and `editor/paste` P4, P7, P14.2: a pasted bitmap's name added twice (reused), the same bytes as `.png`, a decoded data image as `.gif`, a `paste-<sha1>.jpg` already present with other bytes (hash suffix) |
| `editor/paste/store/fetched-names` | S6, S6.1: every name of S6's table as a desired name, `Pic.PNG` against a present `pic.png` with other bytes, the page editor's `a/b.png` and `a?b.png`, the transform of the lowercase-invariant names |
| `editor/paste/store/snippet-save` | S7: each snippet form of both paths and Meguri's, decoded as the save does, its references and its display encoding |
| `editor/paste/url/names` | S6's rows (page column) fetched from the fixture server, `..png`, the whole media-type table of `editor/paste` P10.3 and types that do not match it |
| `editor/paste/url/status` | S5's page column: 2xx statuses, an empty body, 404, 500, an unknown path, redirects (10 followed, 11 refused), the name after a redirect (S6), and the 3xx that is not followed (a 302 without `Location`, a 304 with a body sent) |
| `editor/paste/url/timeout` | S5's page column: 31 s before the first byte, and a body that takes 33 s with no 30 s gap |
| `editor/paste/url/stored-name` | S6.1's three rows |
| `editor/paste/url/file` | S5's `file://` rows and S6's `file://` row (page column); the encoded `#` against a present file `a` (the page editor reads `a` and names it `a#b.png`) and against a missing truncated path |

The host editor's fetch runs in the desktop process, outside the collection library, and the oracle cannot observe it. Meguri's expected value for every step of these scenarios is `specs/conformance/editor-ops.md` E5.1. Everything else in this file happens in the editor and is unobservable by that oracle too: S2 to S4, S5's and S6's host column, S8's encoding, S9 to S13. For those Meguri's tests (U197: `PasteProcessorTests`, `PlainTextToHTMLTests`, `RemoteMediaFetcherTests`) take the tables of this file as their expected values, and the editor-page oracle of `editor/paste` P1 records goldens for `editor/paste` P24's scenarios from both paths (S1).

### S15.1 The fetch op

`specs/conformance/editor-ops.md` E2 to E4 define `add_media_from_url` and its setup sections `http_fixtures` and `url_files`. Its output is the name the call returned: on the oracle the asked-for name of S6.1, on Meguri the stored name, so the rows where they differ carry `editor-paste-url-stored-name`. Every failure is the one rejection `fetch_failed`: the call reports a refused URL or name and a failed download alike, as a localized text only, so the earlier draft's separate `invalid_url` kind cannot be told apart and was dropped (2026-11-10).

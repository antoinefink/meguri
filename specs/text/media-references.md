# Media references in field text

Spec: text/media-references
Requirements: R987, R988, R989 (reference scan used by Check Media)
Anki: 26.09.3
Scenarios: conformance/scenarios/text/refs/, conformance/scenarios/text/corpus/

Which media file names a field references, and the percent-encoding of local names in HTML shown in web views. Fields store raw, unencoded names (R978). Pattern conventions (case-insensitivity, word characters, "shortest", leftmost-first) are `strip-html.md` H1.

## M1. Terms

- A **media tag** is an HTML element that names a media file in an attribute (M2). A **sound tag** is `[sound:<name>]` (M3).
- The **raw name** is the name exactly as written in the field. The **decoded name** is the raw name after entity decoding (`strip-html.md` H3 applied to the name alone).

## M2. Media tag grammar

A media tag match is, in order:

1. `<` immediately followed by a tag name: `img`, `audio`, `video`, `object` or `source`, case-insensitive, where the character after the name is not a word character. (`<imgx`, `<img2`, `<image`, `<img` + `é` do not match; `<IMG`, `<ſource` with U+017F, `<img` + U+00A0 do.)
2. An attribute run: one or more units, as few as possible, where a unit is one character other than `>`, or a quoted run: `"` + one or more characters other than `"` + `"`, or the same with `'`. A single character is preferred over a quoted run, so a quoted run is used only to step over a `>` inside quotes.
3. The attribute: `src` or `data`, case-insensitive, with a non-word character right before it and `=` right after it. So `data-src=` counts (through its `src=`), while `srcset=`, `xsrc=` and `src =` do not.
4. The value, the first of these that matches:
   1. `"` + the name: one or more characters other than `"`, as few as possible + `"` + any characters other than `>` + `>`;
   2. the same with `'`;
   3. unquoted: the name, one or more characters other than `>` (spaces allowed), as few as possible, followed either by `>`, or by U+0020 + any characters other than `>` + `>`.

The match runs from the `<` to the `>` that closes the value. Matches never overlap: the leftmost match wins and scanning resumes after its `>`. When no value form completes at the first eligible attribute, the attribute run grows and a later `src=` or `data=` in the same tag may match. A tag with no `>` after its value is no match.

Consequences the scenarios pin down:

| Field text | Raw name |
|---|---|
| `<img src="a b.jpg">` | `a b.jpg` |
| `<img src=unq.png title=t>` | `unq.png` |
| `<img src=unq.png/>` | `unq.png/` |
| `<audio src= [br].png alt="z">` | ` [br].png` (leading space; it ends before the next space) |
| `<img src=a.png` LF `alt=x>` | `a.png` LF `alt=x` (only U+0020 ends an unquoted name) |
| `<img src='a'b'>` | `a` |
| `<img src="">` | `""` (two quote characters, through the unquoted form) |
| `<img alt="src=x.png" src="y.png">` | `x.png"` (the first `src=` is inside the alt text) |
| `<img alt="q>r" src="s.png">` | `s.png` |
| `<img alt='t' > src='u.png'>` | no match |
| `<video poster="p.jpg" src="v.mp4">` | `v.mp4` |
| `<img src="never closed.png"` | no match |

## M3. Sound tag grammar

`[sound:` (case-sensitive), then the name: one or more characters of any kind (`]` and line breaks included), as few as possible, then `]`. So the name ends at the first `]` after its first character: `[sound:a]b]` names `a`, `[sound:]]` names `]`, `[sound:]` alone is no tag. Matches never overlap; scanning resumes after the closing `]`.

## M4. Reference extraction

The references of a text are, in this order: the decoded names of all media tag matches (M2) in text order, then the decoded names of all sound tag matches (M3) in text order. Duplicates are kept. The two scans run independently over the same text, so `<img src="[sound:7.mp3]">` gives `[sound:7.mp3]` and then `7.mp3`.

Decoding is per name and all or nothing (`strip-html.md` H3): `<source src="a&amp;b.ogg">` → `a&b.ogg`; `<img src="a&amp;b&c.png">` → `a&amp;b&c.png`; `<img src="&nbsp;n.png">` → ` n.png` (a space, since U+00A0 becomes U+0020 after a decode).

Extraction lists remote names too (M5). Consumers filter them.

## M5. Remote names

A reference is remote when its raw name starts with `http://` or `https://`, case-insensitive (`HTTPS://` counts; so does an `s` written as U+017F). The test is on the raw name: ` https://...` (leading space) and `http&#58;//...` are local, although the latter decodes to `http://...`. Protocol-relative names (`//host/x`) and `data:` URLs are local.

- The reference scan that decides which files a note uses (Check Media, U130) is M4 without remote references. Media referenced only from templates through `{{Field}}` substitutions is not scanned (R989): only field text is.
- IRI encoding and decoding (M6, M7) leave remote media tags unchanged.

## M6. IRI encoding for display

Applied to field HTML before it is shown in a web view (R988). For each media tag match (M2) whose raw name is not remote:

1. The encoded name is the raw name with each of these ASCII characters replaced by `%` and its two-digit upper-case hex code: U+0000–U+001F, U+007F, space, `"`, `<`, `>`, `` ` `` and `#`. Every other character is kept, `%`, `'`, `&`, `?`, `=`, `+`, `/`, `~` and all non-ASCII characters included. Entities are not decoded first.
2. If the encoded name differs from the raw name, every occurrence of the raw name inside the match text (from its `<` to its `>`) is replaced by the encoded name, left to right, without overlap. Other attributes that contain the name change too: `<img title="a b" src="a b">` → `<img title="a%20b" src="a%20b">`.

Text outside matches, sound tags and remote tags are unchanged. Note that upstream percent-encoding libraries also encode every non-ASCII byte; this encoding must not (`<img src="日本.png">` stays as it is).

Examples: `<img src="a b#c.jpg">` → `<img src="a%20b%23c.jpg">`; `<img src="100%.jpg">` unchanged; `<img src='q"r.png'>` → `<img src='q%22r.png'>`; `<img src="">` → `<img src=%22%22>`; `<img src="http&#58;//x.org/a b.png">` → `<img src="http&%2358;//x.org/a%20b.png">`.

## M7. IRI decoding

The inverse used before saving editor HTML (R988). For each media tag match whose raw name is not remote:

1. Turn each `%` followed by two hex digits (either case) into that byte; any other `%` stays. Read the bytes as UTF-8, replacing each invalid sequence with U+FFFD the way Rust's lossy UTF-8 decoding does (`percent_decode_str(..).decode_utf8_lossy()` of the `percent-encoding` crate behaves exactly so).
2. If the decoded name differs, replace every occurrence of the raw name in the match text, as in M6.

Examples: `%20lead.png` → ` lead.png`; `a%2Fb.png` → `a/b.png`; `%e6%97%a5.png` → `日.png`; `%ff.png` and `%E6%97.png` → U+FFFD + `.png`; `a%2.png` unchanged; `a%00b.png` → `a` + U+0000 + `b.png`.

## M8. Round trip

Decoding (M7) the encoding (M6) of a text gives the text back when the text meets all four conditions below. Conditions 1 to 3 apply to every local media tag match of the text; condition 4 compares two scans.

1. The raw name contains no `%` followed by two hex digits. Otherwise decoding turns that sequence into a character: `<img src="a%20b.jpg">` encodes to itself and decodes to `<img src="a b.jpg">`.
2. The encoded name does not already occur elsewhere in the match text. Otherwise decoding replaces that copy too: `<img title="a%20b" src="a b">` encodes to `<img title="a%20b" src="a%20b">`, which decodes to `<img title="a b" src="a b">`.
3. The raw name occurs only once in the match text. Encoding replaces every occurrence, also one that is part of the tag's own syntax: `<img src="<">` encodes to `%3Cimg src="%3C">`, which has no media tag, so decoding leaves it unchanged.
4. Scanning the encoded text (M2) finds as many media tag matches as scanning the text, and the n-th match of the encoded text names the encoded name of the n-th match of the text (a remote name stays as it is). Encoding can change which tags match. A `>` or `"` in a quoted name can be what keeps the attribute run (M2 step 2) of an earlier tag without a value of its own from reaching a later value; encoding removes that character, and the earlier tag's match then runs on. `<video title="a">x<img src="a>b.png"> <audio title="t" src="https://e.org/p.png">` has two matches, the `img` tag (`a>b.png`) and the remote `audio` tag. Its encoding, `<video title="a">x<img src="a%3Eb.png"> <audio title="t" src="https://e.org/p.png">`, has one match: it starts at `<video`, runs over the `img` tag and names the remote `audio` file. So decoding changes nothing, and `a%3Eb.png` stays encoded.

Property tests over the corpus must restrict themselves to texts meeting all four conditions.

## M9. Scenarios

- `refs/extract` (M2–M5, and the matches of the M8 condition 4 example and its encoding), `refs/iri` (M5–M8: the M8 examples, each with the text its encoding gives), and the 48,000 distinct generated texts of `corpus/fields-*`.
- Observation functions (`specs/conformance/observations.md` O9): `media_refs` (M4, remote names included), `iri_encode` (M6), `iri_decode` (M7).

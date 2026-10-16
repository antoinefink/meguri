# Anonymized clone

Spec: fixtures/anonymizer
Requirements: R1941, R1915; supports R1938, R1939, R1940, R1942
Anki: 26.09.3
Scenarios: none. The anonymizer is Meguri's own tool. What it must keep of Anki's behavior (Check Database results, next states, queues) is checked on whole collections with `tools/mg oracle check --schedule` (`specs/conformance/collection-check.md` K11), as A17 V3 runs it; its own rules are checked by the synthetic-input tests of A20.

The anonymizer turns a copy of a collection (the user's, through the private vault, or any other) into a clone that holds none of its text, deck names, tag names or media file names, and that Anki schedules exactly as the original: same cards, same review history, same next states, same queues. Field text keeps its shape (HTML, clozes, furigana, media references, lengths) so the clone can stand in for the original in rendering and performance work (R1915). The clone is still derived from private data: it stays under `build/private/` unless the user decides otherwise.

## A1. Terms

- **Source**: a directory holding `collection.anki2`, optionally `collection.anki2-wal` and `collection.media/`. The anonymizer only reads it.
- **Clone**: the output directory: `collection.anki2`, `collection.media/` and `anonymize.json` (A16).
- **Salt**: 32 secret bytes that key every hash and random draw (A3).
- **Kept** text is copied byte for byte. **Replaced** text goes through the character rule of A5.
- **Fold**: the case-folded form under `text/unicode-primitives#U2`: each character replaced by its folding, in the order U2 compares them, written as UTF-8. Two strings are equal under U2 exactly when their folds are equal. `UniCase::new(s)` of `unicase` =2.6.0 feeds exactly these bytes to a `Hasher`, ASCII and non-ASCII strings alike.
- **Name map**: the function from source media file names to clone names (A9.1).

## A2. Entry points

### A2.1 Library

`meguri_testkit::anonymize` takes a source directory, an output directory and a salt, and returns the report of A16 or an error of A18. It writes only inside the output directory.

### A2.2 `meguri-cli anonymize`

```
meguri-cli anonymize --copy <source dir> --out <clone dir> --salt-file <file>
```

- `--copy`: the source (A1). It is called a copy because the only private source is a per-run vault copy (R1938); the command never writes it.
- `--out`: must not exist or be an empty directory, and must not be the source or lie inside it (A18 `out_not_empty`, `usage`).
- `--salt-file`: a file holding the salt as 64 lower-case hex digits, optionally followed by one LF. When the file does not exist, the command creates it with 32 random bytes from the operating system's generator, readable and writable only by its owner, and uses them. Any other content is A18 `salt`.
- On success it writes the clone and prints `anonymize.json`'s path. On failure it exits non-zero and names the A18 kind; it deletes everything it wrote under `--out`.

### A2.3 `tools/mg anonymize`

```
tools/mg anonymize [--source <dir>] [--out <dir>] [--salt-file <file>] [--now <instant>] [--tz <zone>]
```

- `--source`: default, a fresh per-run vault copy of `local-data/anki/` (U15 vault; the vault's rules apply: no CI marker, no remote machine, only the collection file and media folder). Any other directory, such as `build/fixtures/<...>-10k/`, is used as it is, read-only.
- `--out`: default `build/private/<run id>/clone/`. `--salt-file`: default `build/private/anonymize.salt`, kept across runs so that the same source gives the same clone.
- `--now` and `--tz`: the instant and zone of the V3 oracle checks, in `collection-check.md` K1's forms. Defaults: from the source's `manifest.json` by K1's rules when it has one; otherwise 12:00 of today in the host's zone, and the host's IANA zone.
- Runs `meguri-cli anonymize` (V1, V2, V4, V5 included), then V3, and writes the job's `result.json`: `pass` when every check passes, `warn` when only V4 review items remain, `fail` otherwise; `details` holds the report and the V3 comparison. The clone directory is listed as an artifact; nothing is written outside `build/`.

## A3. Salt, hashes and random draws

### A3.1 Hash

`H(domain, key)` = the first 8 bytes of SHA-256(salt ‖ domain ‖ 0x00 ‖ UTF-8 of key), written as 16 lower-case hex digits. Domains are the ASCII strings `meguri-anonymize/media` and `meguri-anonymize/tag`.

### A3.2 Random draws

Every random character and placeholder byte comes from a ChaCha12 stream (`rand` =0.9.4 `StdRng::from_seed`) whose 32-byte seed is SHA-256(salt ‖ `meguri-anonymize/stream` ‖ 0x00 ‖ UTF-8 of a place key):

| Place key | Draws for |
|---|---|
| `note:<id>` | the note's fields, in field order |
| `deck:<id>` | the deck's description, then its search terms in order |
| `config:<KEY>` | the value of a rewritten config key (A13.5) |
| `media:<clone name>` | the bytes of a random placeholder (A10.1) |

A choice among `n` candidates is one `random_range(0..n)` draw. So the clone depends only on the source and the salt, and one note's text never depends on another's.

### A3.3 Determinism

The same source and salt give a byte-identical clone (`collection.anki2`, every media file and `anonymize.json`). Another salt gives other hashes and other text with the same structure.

## A4. What changes

| Table | Columns | In the clone |
|---|---|---|
| `col` | all | unchanged |
| `notes` | `id`, `guid`, `mid`, `mod`, `usn`, `flags`, `data` | unchanged |
| `notes` | `flds` | each field per A5–A9; fields stay separated by U+001F and keep their count |
| `notes` | `tags` | A12.2 |
| `notes` | `sfld`, `csum` | recomputed (A14.1) |
| `cards`, `revlog`, `graves` | all | unchanged |
| `decks` | `id`, `mtime_secs`, `usn`, `common` | unchanged |
| `decks` | `name` | A11.1 |
| `decks` | `kind` | unchanged except a normal deck's description and a filtered deck's searches (A11.3) |
| `deck_config` | all | unchanged (preset names included) |
| `notetypes`, `fields`, `templates` | all | unchanged (note type, field and template names, templates and CSS included) |
| `tags` | `tag` | A12.3; `usn`, `collapsed`, `config` unchanged |
| `config` | `KEY`, `usn`, `mtime_secs` | unchanged |
| `config` | `val` | unchanged except the keys of A13.5 |
| `sqlite_stat1` | all | unchanged |
| `sqlite_stat4` | all | unchanged except the rows A14.2 removes |
| any other table | all | unchanged |

The media folder changes per A9 and A10. Nothing else from the source reaches the clone.

## A5. Character replacement

### A5.1 Kept and replaced characters

A character is replaced when it is a letter, a mark or a number: Unicode general category L, M or N, as the `regex` =1.11.2 classes `\p{L}`, `\p{M}` and `\p{N}` define them. Variation selectors (U+FE00–U+FE0F, U+E0100–U+E01EF) are kept although they are marks. Every other character is kept: whitespace, punctuation, symbols (emoji included), control and format characters (TAB, LF, U+200D), private-use and unassigned characters.

So word boundaries, line breaks, punctuation and every syntax made of punctuation (HTML delimiters, `{{`, `::`, `}}`, brackets, search operators) survive.

### A5.2 Replacement pools

Each replaced character becomes one character drawn uniformly from the pool of its class. Every pool character has the same UTF-8 length as the characters of its class, and none is a combining mark, a Hangul jamo or a character that NFC changes, so the result stays NFC (`text/unicode-primitives#U3`) when the input was.

| Replaced character | Pool |
|---|---|
| `a`–`z` | `a`–`z` (A5.4 removes one letter in two places) |
| `A`–`Z` | `A`–`Z` |
| `0`–`9` | `0`–`9` |
| Hiragana U+3041–U+309F | U+3041–U+3096 |
| Katakana U+30A1–U+30FF | U+30A1–U+30FA |
| Han: U+3005, U+3007, U+3021–U+3029, U+3400–U+4DBF, U+4E00–U+9FFF, U+F900–U+FAFF | U+4E00–U+9FA5 |
| Hangul syllables U+AC00–U+D7A3 | U+AC00–U+D7A3 |
| any other, 2 UTF-8 bytes, category Lu or Lt | U+00C0–U+00D6, U+00D8–U+00DE |
| any other, 2 UTF-8 bytes | U+00DF–U+00F6, U+00F8–U+00FF |
| any other, 3 UTF-8 bytes | U+4E00–U+9FA5 |
| any other, 4 UTF-8 bytes | U+20000–U+2A6D6 |

Script is kept for Latin ASCII, kana, Han and Hangul, case for Latin; other scripts keep only their byte length and case. Kana staying kana is what keeps furigana readings readings (A8).

### A5.3 Entities

An entity is `&` followed by a valid entity per `text/strip-html#H3`'s grammar (an HTML 4 name, `#` and decimal digits, or `#x` and hex digits, then `;`), judged at that position alone.

- An entity whose character is kept (A5.1) is copied as written: `&nbsp;`, `&amp;`, `&lt;`, `&gt;`, `&quot;`, `&#39;`, `&#x2014;`.
- An entity whose character would be replaced becomes one literal character drawn for that character (A5.2): `&eacute;` becomes a character of the 2-byte lower-case pool, `&#20320;` a Han character.
- An `&` that does not start a valid entity is ordinary punctuation, kept.

### A5.4 Guards

Replacement never creates a syntax token the source did not have:

- G1. A replacement for `a`–`z` that directly follows `{{` in the output is drawn from `a`–`z` without `c` (no new cloze opener, A7).
- G2. A replacement for `a`–`z` that directly follows `[soun` in the output is drawn from `a`–`z` without `d` (no new sound tag).
- G3. When the source has an `&` that does not start a valid entity, followed by ASCII letters, digits and `#` up to a `;`, and the replaced run from that `&` to the `;` forms a valid entity, the run's replaced characters are drawn again, in order, until it does not.

### A5.5 Length

A replaced character becomes one character of the same UTF-8 length and a replaced entity becomes one character; everything else is copied. So a field keeps its character count and byte length except where replaced entities shrink it, and every kept token stays where it was relative to the text around it.

## A6. Field HTML

Fields are read with the tokenizer below, never with an HTML parser that repairs markup. A field is scanned left to right after the media references of A9.2 have been located; a located name is replaced by its clone name and the rules below apply to every character outside located names.

### A6.1 Tokens

| At | Token | In the clone |
|---|---|---|
| `<!--` | comment: up to and including the first `-->`, or to the end of the field | `<!--` and `-->` kept; content as text (A6.3) |
| `<` + ASCII letter | start tag: the tag name (characters up to whitespace, `/` or `>`), then attributes up to the `>` that ends the tag, a `>` inside a quoted value not ending it | tag name, attribute names, `=`, quotes, whitespace and `/` kept; values per A6.2 |
| `</` + ASCII letter | end tag up to the first `>` | kept |
| `<!` (not a comment) or `<?` | up to the first `>` | delimiters kept; content as text |
| after a `style` start tag | raw content up to the first ASCII case-insensitive `</style`, or the field's end | kept, CSS `url()` names per A9.2 |
| after a `script` start tag | raw content up to the first `</script` | as text |
| anything else | text run up to the next `<` that starts one of the forms above | as text |

An attribute is a name (characters other than whitespace, `/`, `>`, `=`), optionally followed by `=` and a value: `"` to the next `"`, `'` to the next `'`, or unquoted up to whitespace or `>`. A tag or `<!`/`<?` form with no closing `>` before the field's end is not a tag: the `<` and what follows are a text run.

### A6.2 Attribute values

- `style`: kept, except the names inside CSS `url(...)` (A9.2).
- `class`, `id`, `dir`, `lang`, `align`, `valign`, `width`, `height`, `size`, `color`, `bgcolor`, `face`, `border`, `cellpadding`, `cellspacing`, `colspan`, `rowspan`, `span`, `start`, `type`, `role`, `tabindex`, `target`, `rel`, `controls`, `autoplay`, `loop`, `muted`, `preload`, `contenteditable`, `draggable`, `hidden`, `spellcheck`, `translate` (names compared ASCII case-insensitively): kept.
- Any other attribute (`alt`, `title`, `href`, `value`, `data-*`, `aria-*`, event handlers, ...): when the fold of the whole value's NFC form, after entity decoding, is the key of a file of the source media folder (A9.1), the value becomes that file's clone name; otherwise the value is replaced as text (A6.3), except that a value starting with `http://` or `https://` (ASCII case-insensitive) keeps those letters, and a value starting with `data:` keeps everything up to and including its first `,`.

### A6.3 Text

"As text" applies, at each position, the first of these that matches there: the cloze rule (A7), located media names (A9.2), LaTeX markers, bare media names (A9.2), the bracket rules (A8), entities (A5.3), then the character rule (A5.1–A5.4). LaTeX markers `[latex]`, `[/latex]` (ASCII case-insensitive), `[$]`, `[/$]`, `[$$]` and `[/$$]` are kept; what they enclose is text.

## A7. Clozes and image-occlusion data

Card generation reads cloze numbers from the raw field text, tags and comments included, so cloze syntax is kept wherever it appears in text (text runs, comments, replaced attribute values, script content):

- An opener shape is `{{c`, one or more ASCII digits or commas, then `::`. Every opener shape is kept verbatim, whether or not Anki accepts it as a cloze (`{{c1::`, `{{c1,2::`, `{{c,::`, `{{c99999::` alike). The number list is never touched.
- `}}` and `::` are punctuation and stay. So nesting (`{{c1::a {{c2::b}} c}}`), hints (`{{c1::answer::hint}}`, where both the answer and the hint are replaced as text and the `::` stays), several hints, unbalanced openers and stray `}}` all keep their shape.
- G1 keeps replaced text from forming a new opener: `{{x1::` never becomes `{{c1::`.
- Image-occlusion data: when an opener shape is directly followed by `image-occlusion:`, the text from there to the next opener shape or `}}` is shape data: `image-occlusion:`, then a shape name, then properties `:<name>=<value>`, where a value runs to the next `:` not escaped by `\` (`\:` and `\\` are escapes inside a value). All of it is kept (shape names, coordinates, `oi`, colors) except the value of each `text` property, which is replaced as text (its `\` escapes are punctuation and stay). This applies in any note.

Example: `{{c1::der Hund::animal}}` becomes `{{c1::` + 3 lower-case letters + ` ` + 1 upper-case and 3 lower-case letters + `::` + 6 lower-case letters + `}}`: the answer keeps its 8 characters and its space.

## A8. Furigana and bracket annotations

- Furigana (`日本語[にほんご]`, ` 漢字[かんじ]`): the brackets and the space before the base are punctuation and whitespace, the base is Han and the reading kana, so A5 keeps the syntax and the scripts; readings stay as long as before.
- Bracket annotation codes: in a text run, a `[` ... `]` pair on one line, with no other `[` or `]` inside and not a sound tag, is an annotation. When the part after its first `;` or `,` consists only of items separated by `;` or `,`, each item 1 to 5 characters of `a`–`z` and `0`–`9`, that part is kept: it is a reading or part-of-speech code, not content (`出[だ;k1]`, `olla[olla,n,f]`, `Hund[Hund,nn,m]` keep `;k1`, `,n,f`, `,nn,m`). The part before it is replaced as text. Any other annotation is all text.

## A9. Media references

### A9.1 Name map

- Domain: every regular file directly inside the source's `collection.media/` (entries that are not regular files, and everything inside subdirectories, are skipped and counted, never copied), and every well-formed local name that a field references (A9.2), whether or not the file exists.
- A name is well-formed when its decoded form (`text/media-references#M1`) is not empty, is not remote (`#M5`) and contains none of the characters `text/media-filenames#F1` step 2 removes (`[ ] < > : " / ? * ^ \ |`, control characters). A data URL is not well-formed (it has `:`).
- Key: the fold (A1) of the NFC form of the decoded name.
- A name whose first character is `_` maps to itself: Anki's convention for files that templates use (`_font.woff2`, `_back.png`); the templates stay unchanged and must find them.
- Every other name maps to `H(meguri-anonymize/media, key)`, followed by `.` and the extension when there is one: the text after the name's last `.`, when it is 1 to 10 ASCII letters or digits, written in ASCII lower case. Clone names are lower case and unchanged by `text/media-filenames#F1`.
- Two source files with the same key: A18 `media_name_collision`. Two keys with the same clone name: A18 `hash_collision`.

### A9.2 Where references are found and rewritten

- Media tags and sound tags: every match of `text/media-references#M2` and `#M3` in the raw field text. A well-formed local name is replaced by its clone name, raw form for raw form (the clone name needs no entity encoding). The rest of the match (`[sound:`, `]`, the tag and its other attributes) follows A6. Any other match is not a reference: its name is processed by A6 like the text around it, with these exceptions in attribute values: a remote name keeps its `http://` or `https://`, and a data URL keeps everything up to its first `,`.
- CSS: in kept `style` values and style blocks, each `url(` ... `)` (optional whitespace and one pair of `"` or `'` around the name): a well-formed local name is replaced by its clone name; a remote name keeps its scheme and the rest is replaced as text; a data URL keeps everything up to its first `,` and the rest is replaced as text.
- Bare names: in a text run or a replaced attribute value, a maximal run of non-whitespace characters, the fold of whose NFC form is the key of a file in the source media folder and which has an extension (A9.1), is replaced by that file's clone name. Fields that hold only a file name, which templates wrap as `[sound:{{Audio}}]` or `<img src="{{Image}}">`, keep working.
- A referenced file that does not exist stays referenced under its clone name and stays missing.

Example: `<img src="Photo.JPG">` and a file `photo.jpg` both map to the same clone name; `[sound:dog.mp3]` becomes `[sound:<16 hex>.mp3]`.

## A10. Media files

### A10.1 Placeholders

Every regular file of the source media folder is written into the clone's `collection.media/` under its clone name, as the first of these that applies says:

- a file whose name maps to itself (`_`): its bytes unchanged;
- an image-occlusion image (A10.2): a valid image (A10.3, A10.4);
- any other file: as many bytes as the source file, drawn from the stream `media:<clone name>` (sounds, videos, fonts and pictures alike are not decodable).

### A10.2 Image-occlusion images

A note is an image-occlusion note when its note type's config has `original_stock_kind` 6. Every file referenced (A9.2, well-formed) by any field of such a note whose extension, ASCII case-insensitive, is `jpg`, `jpeg`, `png`, `gif`, `webp`, `ico`, `avif` or `svg` gets a valid placeholder. The image-occlusion review page sizes its canvas and masks from the image's natural size, so the placeholder has the source image's pixel dimensions:

| Source file starts with | Width and height |
|---|---|
| PNG signature `89 50 4E 47 0D 0A 1A 0A` | IHDR: big-endian 32-bit at bytes 16–19 and 20–23 |
| `GIF87a` or `GIF89a` | little-endian 16-bit at bytes 6–7 and 8–9 |
| `FF D8` (JPEG) | walking the segments from byte 2 (a marker, then a big-endian 16-bit length that counts itself), the first SOF marker (`FF C0`–`FF CF` except `C4`, `C8`, `CC`): after its length and 1 precision byte, height then width, big-endian 16-bit |
| `RIFF`, 4 bytes, `WEBP` | first chunk `VP8X`: canvas width − 1 and height − 1, 24-bit little-endian at chunk data bytes 4–6 and 7–9; `VP8L`: after the `2F` byte, width − 1 in the next 14 bits and height − 1 in the 14 after, least significant bit first; `VP8 `: after a 3-byte frame tag and `9D 01 2A`, 16-bit little-endian width and height, low 14 bits each |
| anything else, a missing file, or a dimension of 0 | 1 × 1 |

### A10.3 PNG placeholder

For every extension but `svg`, the placeholder is a black PNG of width W and height H whatever the extension says (`jpg`, `webp` and the rest included): Anki opens image-occlusion images by extension, and the webviews pick the decoder from the bytes.

1. Signature, then `IHDR` (W, H, bit depth 1, color type 0, compression 0, filter 0, interlace 0), one `IDAT`, `IEND`; each chunk with its CRC-32.
2. The raw image data is H rows of one filter byte 0 and ceil(W / 8) zero bytes: L = H × (1 + ceil(W / 8)) zero bytes.
3. `IDAT` holds a zlib stream: header `78 01`, one deflate block (RFC 1951) with BFINAL 1 and fixed Huffman codes (BTYPE 01), then the Adler-32 of the raw data, big-endian: `(L mod 65521) × 65536 + 1`.
4. The block encodes the L zero bytes as: the literal 0; then, while at least 258 bytes remain, a length-258 match at distance 1; then one match of the remaining length at distance 1 if it is 3 or more, or 1 or 2 literal zeros; then end-of-block. Codes per RFC 1951 §3.2.6: literal 0 is `00110000`, end-of-block `0000000`, length 258 is symbol 285 (`11000101`, no extra bits), other lengths their symbol and extra bits, distance 1 is distance code 0 (`00000`, no extra bits). Bits fill each byte from its least significant bit (RFC 1951 §3.1.1); Huffman codes enter the stream most significant bit first, header fields and extra bits least significant bit first.

| W × H | PNG bytes | SHA-256 |
|---|---|---|
| 1 × 1 | 67 | `836c5e8c94b74be78456122528bb2a44b4cf61b3922f211e2bae5bf327f95f09` |
| 3 × 2 | 67 | `d3bfdfaae80e2d8dfd859fb9f4d382a6368fde337ddea9dd7d79a6e32a143176` |
| 100 × 100 | 76 | `44e90f2c74614e596431e6781d61332c5753102570825f8649066c6b196e314f` |
| 2000 × 1500 | 2,439 | `35cee25202bbfc7757a71169d17778f7abd76d81da56afb050a7c1bbb2c7d9bc` |
| 4000 × 3000 | 9,534 | `10cd98720f649dd5d1897222b639f89f1100a77b95618fbf925903777b79d179` |

When the PNG is shorter than the source file, zero bytes follow `IEND` up to the source's size (decoders stop at `IEND`). When it is longer, it is written as it is and counted (A16 `io_images_larger`).

### A10.4 SVG placeholder

`<svg xmlns="http://www.w3.org/2000/svg"`, then ` width="…"`, ` height="…"` and ` viewBox="…"` with the values of those attributes on the source's first `<svg` start tag (read per A6.1) when present and made only of ASCII digits, spaces and `.,+-eE%px`; ` width="1" height="1"` when none of the three is there; then `/>`. Spaces (U+0020) follow up to the source's size; a longer placeholder is counted as in A10.3.

## A11. Decks

### A11.1 Names

Sibling order decides Anki's queues: new cards are gathered deck by deck in the order of the decks' full names under U2 (the default gather order), and two review orders sort by deck. A hash would reorder siblings, so deck names are replaced by ranks:

1. Build the tree of name paths: every deck's full name and every prefix of it at a `::`, paths being equal when their folds are (missing parents included: a deck `A::B` with no deck `A` still has the path `A`).
2. Under each path (and at the top), sort the child components by U2. The i-th child (from 0) of n gets the component `d` followed by i in decimal, zero-padded to the number of digits of n − 1 (one digit up to 10 children).
3. A deck's new name is its path's new components joined by `::` (in `decks.name`, by U+001F as stored).

Full names, compared under U2, order exactly as their component sequences do, because `::` (stored U+001F) sorts below every character a component can hold; so the clone's decks sort, under U2, in the same id order as the source's. The default deck (id 1) is renamed like any other: Anki recognizes it by id. Ids never change.

Example: decks `Default`, `german`, `Japanese`, `Japanese::Audio`, `Japanese::Grammar` become `d0`, `d1`, `d2`, `d2::d0`, `d2::d1`.

### A11.2 Uniqueness

Ranks are distinct under each path, so clone names are unique under U2 (the `decks` name index) by construction.

### A11.3 The `kind` blob

`decks.kind` holds a `KindContainer` protobuf: field 1 `normal`, field 2 `filtered` (`model/protobuf-records`).

- `normal`: its `description` (field 4) is rewritten as field text (A6–A9, place `deck:<id>`); `markdown_description` and everything else stay.
- `filtered`: the `search` (field 1) of each `search_terms` entry (field 2) is rewritten per A13.
- Only those string values and the length prefixes that enclose them change, each length a minimal varint. Every other byte, unknown fields included, stays in place.

## A12. Tags

### A12.1 Names

- A tag is split into components at `::`.
- A first component whose fold is `leech` or `marked` is kept as written: Anki gives these two tags meaning.
- Every other component becomes `H(meguri-anonymize/tag, fold of its NFC form)`.
- The clone tag is the components joined by `::`. Equal components give equal hashes, so parent and child relations are unchanged, and tags equal under U2 (`German`, `german`) become one name.

Examples (salt A19): `jp::n5` → `97ed61c408f64a10::56fcd14408509d6b`; `JP::N5` → the same; `Straße` and `STRASSE` → `4d6b561dad60f313`; `german::verb` → `86dd12acb57c5398::87bee7bf6e40e099`, `spanish::verb` → `b347e7b99054d409::87bee7bf6e40e099`; `leech` → `leech`.

### A12.2 Note tag strings

`notes.tags` is written in Anki's canonical form: the note's tags (split at U+0020 and U+3000) mapped, duplicates under U2 dropped, sorted by U2, joined by single spaces, with one space before and after; the empty string when there are none. A source note whose string was not canonical gets the canonical form; Anki's Check Database would rewrite it the same way in the source (A17 V3 allows this).

### A12.3 `tags` table

Each row's `tag` is mapped as in A12.1. Distinct rows stay distinct: a clash is A18 `hash_collision`.

## A13. Search strings and other config

Searches name decks and tags. A filtered deck's search runs only when the deck is built or rebuilt: the clone's filtered decks hold the same cards as the source's at every instant, whatever their searches say. The searches are rewritten so that a rebuild gathers the same cards wherever the search depends only on deck and tag names.

### A13.1 Reading a search

A search string is read as a sequence of:

- whitespace (any Unicode whitespace), `(` and `)`, kept;
- `-` directly before a term or `(`, kept;
- terms: a quoted term (`"` ... `"`, at least one character inside, `\` making the next character literal), a partly quoted term (a key of characters other than `"`, `(`, `)`, `:` and whitespace, then `:`, then a quoted part), or an unquoted term (up to whitespace or an unescaped `"`, `(` or `)`; `\` escapes the next non-whitespace character). An unquoted `and` or `or` (ASCII case-insensitive) is an operator, kept.

A term's text (inside the quotes for a quoted term) has a key when it holds an unescaped `:`: the key is the text before the first such `:`, the value the text after it. A string that does not read this way (an unclosed quote, an empty quoted term, unbalanced parentheses, an odd `\` at the end, a term whose text starts with `:`) is replaced as text as a whole and counted `searches.unparsed`.

### A13.2 Terms

Keys compare ASCII case-insensitively.

| Term | In the clone |
|---|---|
| no key | value replaced as text |
| `deck:` with value `*`, `filtered` or `current` | kept |
| `deck:` other | A13.3 |
| `tag:` with value `none` or `*` | kept |
| `tag:` with a value starting `re:` or `nc:` | prefix kept, the rest replaced as text; counted `searches.inexact` |
| `tag:` other | A13.4 |
| `note:`, `card:`, `flag:`, `resched:`, `prop:`, `added:`, `edited:`, `introduced:`, `rated:`, `is:`, `did:`, `mid:`, `nid:`, `cid:`, `preset:`, `has-cd:` | kept (note type, template and preset names are kept, A4) |
| `dupe:<id>,<text>` | id and comma kept, text replaced |
| any other key (`re`, `nc`, `sc`, `w`, a field name or field pattern) | key kept, value replaced as text |

Replacing as text keeps quotes, backslashes and wildcards (punctuation), so the term still reads the same way.

### A13.3 Deck terms

- The value is unescaped: `\"`, `\\`, `\:`, `\(`, `\)` and `\-` become the character; `\*` and `\_` stay escapes for the pattern.
- As a pattern, `*` matches any run of characters (`::` included), `_` any one character, `\*` and `\_` a literal `*` and `_`; other characters match themselves with the `regex` crate's `(?i)` case-insensitivity; `::` stands for the separator between components.
- A deck matches when its full name matches the pattern from its start and the match ends at the name's end or right before a `::`. Every descendant of a matching deck matches too.
- The term becomes `deck:<new full name>` when one matching deck has no matching parent, `(deck:<a> OR deck:<b> OR ...)` when several do (in U2 order of the new names), and `did:0` (matches nothing) when none match. A `-` before the term stays before the replacement. Counted `searches.rewritten`.

### A13.4 Tag terms

- The value is unescaped as in A13.3. As a pattern, `*` matches any run of non-whitespace characters and `_` one; the rest as in A13.3. A value containing a space matches nothing.
- The candidate tags are the distinct tags (under U2) on notes and in the `tags` table. A tag matches when it matches the pattern from its start and the match ends at the tag's end or right before a `::`. Descendants of a matching tag match.
- The term becomes `tag:<new>` or `(tag:<a> OR tag:<b> OR ...)` over the matching tags none of whose ancestors match, in U2 order of the new names, or `did:0` when none match. Counted `searches.rewritten`.

Example (decks A11.1, tags A12.1): `deck:Japanese tag:jp::n5 is:due Hund` → `deck:d2 tag:97ed61c408f64a10::56fcd14408509d6b is:due ` + 4 random letters; `deck:*a*` → `(deck:d0 OR deck:d1 OR deck:d2)`; `-deck:Nowhere` → `-did:0`.

### A13.5 Config keys

| Key | Value | In the clone |
|---|---|---|
| `defaultSearchText` | a search string | A13.1–A13.4 |
| `savedFilters` | object: saved search name → search string | each name replaced as text, each search per A13.1–A13.4; a name that would equal an earlier one (members in name order) is drawn again |
| `flagLabels` | object: flag number → label | numbers kept, labels replaced as text |

The rewritten value is written in the compact form of `storage/config-table#C5`; `usn` and `mtime_secs` stay. Every other key is unchanged: `curDeck`, `activeDecks`, `curModel`, `_deck_<id>_lastNotetype` and `_nt_<id>_lastDeck` hold ids, which do not change. A key outside this list that holds names (an add-on's, or `cardStateCustomizer` naming decks) is copied as it is; V4 reports it for review.

## A14. Derived values and the file

### A14.1 Sort field and checksum

`notes.sfld` and `notes.csum` are computed from the clone's fields as a save computes them (`text/checksum-and-sort-field` C1, then C2 and C3 for `sfld`, bound as text so the column's affinity decides the stored type, and C4 for `csum`), with the note type's sort field index.

### A14.2 SQLite statistics

`sqlite_stat1` holds counts only and is kept, so the clone's queries are planned from the source's statistics, stale ones included. `sqlite_stat4` keeps sample index keys: its rows for `idx_decks_name`, `ix_notes_csum` and the `tags` primary key (`sqlite_autoindex_tags_1`), which hold deck names, checksums of first fields and tag names, are deleted; its other rows stay.

Without those samples SQLite may plan a range query on deck names differently. One such query matters to scheduling: the one that lists a studied deck and its subdecks, whose row order the review orders that sort by deck use. The default orders do not; V3 shows any effect on a collection whose presets use them.

### A14.3 Writing the clone

1. Copy `collection.anki2` (and `collection.anki2-wal` when present) into a work directory inside `--out`.
2. Open the copy as a plain SQLite database with the `unicase` collation registered (`storage/schema-18` S4), not through the collection open path: nothing but A4's changes may be written. A collection whose `col.ver` is not 18 is A18 `unsupported_schema`.
3. Apply every change of A4 in one transaction.
4. Write the clone with `VACUUM INTO '<out>/collection.anki2'`: a new file holding only live rows, so no replaced text survives in free pages, cell slack or a WAL. Rowid tables keep their ids.
5. Delete the work directory, also after a failure.

The clone has no `-wal`.

## A15. Scheduling invariants

These must not change, because Anki's queues and next states read them (A17 V3 checks the result):

| Kept | Read by |
|---|---|
| `cards` (all columns) | everything: states, due days, positions (`due` of new cards), `mod` (the random review order hashes card id and `mod`), memory states in `data` |
| card and note ids | random new-card orders hash note or card ids with the day; sibling burying groups by note |
| `revlog` | memory states, intervals, learning steps |
| decks' ids, `common` (today's studied counters) and `kind` limits and preset | daily limits and presets |
| the sibling order of deck names (A11.1) | new cards gathered deck by deck; review orders that sort by deck |
| `deck_config` | steps, limits, orders, FSRS parameters, desired retention, burying |
| `col.crt` and config (`rollover`, `creationOffset`, `localOffset`, `collapseTime`, `fsrs`, `loadBalancerEnabled`, `curDeck`, `activeDecks`, `lastUnburied`, `nextPos`, ...) | the day, learn-ahead, the load balancer, the current deck |
| field emptiness and cloze numbers | the cards a note should have: Check Database adds missing ones |
| `sqlite_stat1` | query plans, so rows with equal sort keys come back in the same order |

Field text, tag names and media do not affect queues or next states. Field emptiness (`text/field-emptiness#E1`) survives because whitespace and spacer tags are kept and any other content stays non-empty.

## A16. Report

`anonymize.json`, in the goldens' text layout (`specs/conformance/dsl.md` D11). Counts only: no text, name or id of the source (R1940).

```json
{
  "checks": {"leak": {"locations": [], "status": "pass"}, "media": "pass", "rows": "pass", "structure": "pass"},
  "chars_replaced": 1520000,
  "decks": {"descriptions": 2, "renamed": 30, "searches": 1},
  "entities_replaced": 12,
  "fields": {"changed": 17000, "total": 20000},
  "format": 1,
  "media": {"bare_names": 40, "files": 9000, "io_images": 20, "io_images_larger": 0, "io_images_unknown_size": 0, "kept": 10, "refs": 9500, "refs_malformed": 1, "refs_missing": 3, "refs_remote": 2, "skipped_entries": 0},
  "notes": 10000,
  "salt_sha256": "<hex>",
  "searches": {"inexact": 0, "rewritten": 2, "unparsed": 0},
  "stat4_rows_deleted": 60,
  "tags": {"kept": 1, "renamed": 120}
}
```

`salt_sha256` is SHA-256 of the salt (it identifies the salt without revealing it). `checks` holds V1, V2, V5 and V4 (A17).

## A17. Validation

- **V1 Structure** (the anonymizer, every field): tokenizing the clone field with A6 gives the source's token sequence, kept tokens byte-equal and each replaced run of the same length with each character in the same A5.2 class (a replaced entity counting as its character); the opener shapes (A7) are the same, at the same character positions; E1 emptiness is the same; the clone's well-formed local references (`text/media-references#M4`) are the source's, mapped. A failure is A18 `structure_changed`.
- **V2 Rows** (the anonymizer): `col`, `cards`, `revlog`, `graves`, `deck_config`, `notetypes`, `fields`, `templates` and `sqlite_stat1` hold the same rows (value and storage class); `notes` and `decks` hold the same ids, each row equal except the columns A4 rewrites; `config` holds the same keys, equal except A13.5's; `tags` holds as many rows.
- **V3 Oracle** (`tools/mg anonymize`): `tools/mg oracle check <source> --schedule` and `tools/mg oracle check <clone> --schedule` with the same `--now` and `--tz` (A2.3). The reports must have equal `counts`, `timing`, `check_db` and `schedule` (`collection-check.md` K11; equal `schedule.sha256`), and every table in the clone's `changed_tables` must be in the source's or be `col` or `config`. A difference fails with the members that differ (for `schedule`, the deck ids whose digests differ). The clone's `check_db` must also show zero problems: a count above 0, or the rejection `{"error": "corrupt"}` (`collection-check.md` K4, K5), fails V3 even when the source's `check_db` is the same, naming the clone's kinds above 0 with their counts, or the rejection. Repair such a source with Anki's Check Database, then anonymize it again. This is the only condition V3 takes from the clone's report alone; the K9 status of either check job is not used.
- **V4 Leak scan** (the anonymizer):
  - Tokens: maximal runs of letters, marks and digits (A5.1) of at least 4 characters taken from the source's deck name components, tag components (A12.1 kept ones excepted), media file names (`_` ones excepted), saved search names and flag labels; dropping runs of only ASCII digits, runs of only hex digits (`0-9a-fA-F`), and runs that occur, under U2, as a run in any note type, field, template or preset name.
  - Scanned: every text and blob value (blobs read as UTF-8, invalid sequences as U+FFFD) of every clone table except `notes.flds` and `notes.sfld` (replaced text, A5) and the unchanged `notetypes`, `fields`, `templates` and `deck_config`; and every clone media file name. A value is split into runs the same way and each run compared with the tokens under U2.
  - A hit in `decks.name`, `tags.tag`, `notes.tags` or a media file name other than a `_` name fails (`checks.leak.status` `fail`): those must be fully rewritten. A hit anywhere else is a review item (`warn`): a location the spec keeps on purpose (a config value, `decks.common`, `cards.data`) may legitimately hold the word. `locations` lists `{table, column, key, hits}` (`key`: the config key or the row id), never the token.
- **V5 Media** (the anonymizer): every clone media file name is a clone name of a source file; sizes equal, except image-occlusion placeholders counted in `io_images_larger`; every PNG placeholder starts with the PNG signature and an IHDR holding the dimensions A10.2 read.

## A18. Errors

| Kind | When |
|---|---|
| `usage` | bad arguments; `--out` is the source or inside it |
| `out_not_empty` | `--out` exists and is not empty |
| `no_collection` | the source has no `collection.anki2` |
| `open_failed` | the copy is not an SQLite database |
| `unsupported_schema` | `col.ver` is not 18 |
| `invalid_utf8` | a note's `flds` is not valid UTF-8 (Anki's Check Database would repair it; run it first) |
| `salt` | the salt file is not 64 lower-case hex digits (and an optional LF) |
| `media_name_collision` | two source media files have the same key (A9.1) |
| `hash_collision` | two different keys, components or tags hash to the same clone name |
| `structure_changed` | V1 failed (the report names the note id; local output only) |

## A19. Examples

Test salt: the 32 bytes `00 01 02 ... 1f`; its `salt_sha256` is `630dcd2966c4336691125448bbb25b4ff412a49c732db2c8abc1b8581bd710dd`.

| Source | Clone |
|---|---|
| media `dog.mp3` | `aae169c91f46c422.mp3` |
| media `Photo.JPG` (or `photo.jpg`) | `8716100710f92e8a.jpg` |
| reference `a&amp;b.png` (file `a&b.png`) | `7c860c6a8a6650f4.png` |
| media `notes.tar.gz` | `29d55ad821dec71d.gz` |
| media `README` | `d2ba4fd1b7428819` |
| media `日本.png` | `8c36fbb97b3ca668.png` |
| media `a b.jpg` | `23abcb0818f98f68.jpg` |
| media `_font.woff2` | `_font.woff2`, bytes unchanged |
| tag `jp::n5` | `97ed61c408f64a10::56fcd14408509d6b` |
| tag `Straße` | `4d6b561dad60f313` |

Field `<b>Hund</b> {{c1::der Hund::animal}} [sound:dog.mp3]` → `<b>` + 1 upper-case and 3 lower-case ASCII letters + `</b> {{c1::` + 3 lower-case letters + ` ` + 1 upper-case and 3 lower-case letters + `::` + 6 lower-case letters + `}} [sound:aae169c91f46c422.mp3]`, and `collection.media/aae169c91f46c422.mp3` has `dog.mp3`'s size.

| Source text | Clone shape |
|---|---|
| `Tom &amp; Jerry&nbsp;<br>` | `Xxx &amp; Xxxxx&nbsp;<br>` |
| `caf&eacute;` | 4 letters, the last from the 2-byte lower-case pool |
| `<span style="color: rgb(255, 0, 0);" title="Note">Rot</span>` | style kept, `title` 4 letters, `Xxx` |
| `<a href="https://example.org/x">x</a>` | `https` kept, the rest of the URL replaced (`://`, `.`, `/` kept) |
| `{{c1,2::a}} {{c1::x {{c2::y}} z}} {{c3::` | every opener and `}}` kept, `a`, `x`, `y`, `z` replaced |
| `{{x1::y}}` | `{{` + a lower-case letter other than `c` + a digit + `::` + a letter + `}}` (not an opener shape, so the digit is replaced too) |
| `{{c1::image-occlusion:rect:left=.25:top=.1:width=.2:height=.1:oi=1}}` | unchanged |
| `{{c2::image-occlusion:text:left=.5:top=.5:text=Leber\:Herz:scale=1}}` | only `Leber` and `Herz` replaced |
| `日本[にほん]は 東京[とうきょう]` | Han, hiragana and brackets in place, readings hiragana |
| `出[だ;k1]す` | `;k1` kept |
| `Cat` (a field holding the name of the file `Cat`) | replaced as text: `Cat` has no extension |
| `dog.mp3` (a field holding a file name) | `aae169c91f46c422.mp3` |

## A20. Tests

`core/crates/meguri-testkit/tests/anonymize.rs` uses only synthetic collections built in the test, with the A19 salt, and takes expected values from this spec:

- the A19 field: the clone's shape, the clone media file and its size, `sfld` and `csum` equal to `meguri-text`'s values for the clone field, and the same output for a second run (A3.3);
- every row of A19's tables, including the hash values;
- decks `Default`, `german`, `Japanese`, `Japanese::Audio`, `Japanese::Grammar` → A11.1's names; 11 siblings → `d00` … `d10`; a deck `A::B` with no `A`;
- tags `jp::n5`, `JP::N5` and `jp` on three notes → one hashed pair, `jp` its parent; `leech` kept; note tag strings canonical (A12.2);
- an image-occlusion note type (`original_stock_kind` 6) with a 3 × 2 PNG and a 3 × 2 JPEG source image: both placeholders are the 3 × 2 PNG of A10.3 padded to their source sizes;
- a filtered deck with search `deck:Japanese -tag:jp*`, a `savedFilters` value and `defaultSearchText`: A13's rewrites; an unparsable search counted;
- V2 on the synthetic collection: unchanged tables equal; `sqlite_stat4` rows of the three indexes gone (after `ANALYZE` on the synthetic source);
- V4: a config value holding a deck name is a review item; a deck name left in `decks.name` (by a test hook) fails;
- errors: `out_not_empty`, `salt`, `invalid_utf8`, `media_name_collision` (two files whose names differ only in case, on a case-sensitive file system).

The whole-collection checks run through `tools/mg anonymize`: the 10k generated collection (`tools/mg fixtures 10k`) gives V3 equality, and the private collection only locally through the vault.

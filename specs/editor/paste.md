# Paste, drop and media insertion

Spec: editor/paste
Requirements: R138, R139, R252, R849, R850, R851, R852, R853, R854, R855, R856, R866, R867, R868, R869, R870, R871, R872, R873, R874, R875, R876, R877, R878, R879, R880, R881, R1960
Anki: 26.09.3
Scenarios: conformance/scenarios/editor/paste/

How content gets into a rich-text field: attaching files, camera and photo-library images, recordings, paste and drop. It covers which pasteboard content wins, how bitmaps are encoded and named, how files enter the media folder, the three HTML filters, what the field holds afterwards, the in-editor image controls and the field context menu. U194 implements P3 to P7, P18 to P20; U197 implements P8 to P17. This file is the paste-filter and paste-sources spec that U197's file list names. The config keys are `editor/config-keys` K2 (`pasteImagesAsPng`) and K3 (`pasteStripsFormatting`); naming in the media folder is `media/media-files` M3 and M4; display encoding of names is `text/media-references` M6 and M7; the field save pipeline is U190's.

## P1. Sources and oracle (R1960)

- Anki 26.09.3 has two editors: the default one, whose paste logic runs in the desktop host, and an experimental one behind a flag, whose paste logic runs in the page. This file follows the default editor unless a rule says otherwise. R867's audio step and R849's bare file name come from the experimental editor; the plan adopts them.
- Decided 2026-10-27, for every editor spec: editor behavior that the pylib oracle cannot observe is specified from Anki 26.09.3 in the spec writer's words, and its goldens are observed by an oracle maintainer who runs Anki's pinned desktop editor (host and page) inside a quarantined oracle image, as `stats/graphs-common` G12 decision 2 does for the graphs page (proposed home: `oracle/editor-page/`). That image is Linux with Qt WebEngine, so goldens are Chromium serializations. The page and its assets never leave the image; goldens hold observed values only. Meguri's app and tests never load Anki's web assets (R1960).
- Behavior the Linux image cannot show (RTF, web archives, Apple pasteboard types, camera, photo library, Quick Look, Finder) is Meguri's own and is tested in Meguri's tests only.

## P2. Terms

- **Strip mode**, **keep mode**: the two modes of an external paste (P8). **Internal mode**: content Meguri produced itself (P9 steps 1, 3 and 4, that is P10 to P13, and P17's drags from a field), filtered by P15.3 only.
- **Snippet**: the markup that stands for one media file in a field (P3).
- **Add** a file: the path of P4, ending in `media/media-files` M3.
- **Media extensions**: R850's lists. Image: `jpg`, `jpeg`, `png`, `gif`, `svg`, `webp`, `ico`, `avif`. Audio and video: `3gp`, `aac`, `avi`, `flac`, `flv`, `m4a`, `mkv`, `mov`, `mp3`, `mp4`, `mpeg`, `mpg`, `oga`, `ogg`, `ogv`, `ogx`, `opus`, `spx`, `swf`, `wav`, `webm`. A name's extension is what follows its last `.` (the whole name when it has no `.`), compared ASCII case-insensitively.
- **Content hash**: SHA-1 of the bytes as 40 lower-case hex digits (`media/media-files` M2).
- **Text whitespace** (P13): the Unicode White_Space characters plus U+001C to U+001F, that is U+0009 to U+000D, U+001C to U+001F, U+0020, U+0085, U+00A0, U+1680, U+2000 to U+200A, U+2028, U+2029, U+202F, U+205F and U+3000.
- **Percent-decoding** (P10, P13): each `%` followed by two hex digits (either case) stands for that byte, every other character for its UTF-8 bytes, and a `%` not followed by two hex digits is kept as it is; `+` is not decoded. The bytes are then read as UTF-8, each maximal ill-formed subsequence becoming one U+FFFD (the WHATWG UTF-8 decoder's rule). Examples: `a%20b` → `a b`; `100%.jpg` → `100%.jpg`; `a+b` → `a+b`; `%E6%97` → U+FFFD; `%FF%FE` → U+FFFD U+FFFD. Decided 2026-11-10.
- **Trim** (P13, P15): remove from both ends every character of this set: U+0009 to U+000D, U+0020, U+00A0, U+1680, U+2000 to U+200A, U+2028, U+2029, U+202F, U+205F, U+3000 and U+FEFF (the web platform's trim). U+0085 and U+001C to U+001F are not trimmed.

## P3. Snippets (R849, R850, R851)

The snippet is chosen from the extension of the name the add returned (the stored name, M3 step 4), never from the name asked for:

| Stored name's extension | Snippet | Example (stored name → field) |
|---|---|---|
| image | `<img src="NAME">` | `photo.jpg` → `<img src="photo.jpg">` |
| audio or video | `[sound:NAME]` as text | `clip.mp3` → `[sound:clip.mp3]` |
| anything else, or none | NAME as text | `notes.pdf` → `notes.pdf` |

- NAME is the stored name exactly. Field text is HTML, so as text `&` is written `&amp;`: `a&b.mp3` → `[sound:a&amp;b.mp3]`. Stored names never hold `<`, `>` or `"` (`text/media-filenames` F1).
- The `img` snippet holds the raw stored name; nothing encodes it when it is built. **Src encoding** happens once, at insertion: P16 for paste and drop, and the same step just before `editor/formatting` F10's insertion for attach, photos and recordings (P5, P6). It writes each UTF-8 byte of the name other than ASCII letters, digits, `_`, `.`, `-`, `~` and `/` as `%` and two upper-case hex digits, so the image loads in the page. The save (`editor/field-html` H3 step 4, `text/media-references` M7) decodes it, so the stored field holds the raw name with no entity escaping, as Anki stores it: `a&b c.png` → `<img src="a&b c.png">`, `日本.png` → `<img src="日本.png">`, `100%.jpg` → `<img src="100%.jpg">`, `a%41.png` → `<img src="a%41.png">`.
- Creating an audio or video snippet plays the file once, at once (R851), whatever made the snippet: attach, record, paste, drop or a download.
- Several snippets from one action are joined with nothing between them: `<img src="a.jpg"><img src="b.jpg">`.

## P4. Adding a file

Every file this spec puts into media goes through one importer (U194), which calls U31's add (`media/media-files` M3) on the maintenance lane and uses the name it returns.

- **Desired name**: a file from disk or a file URL: its last path component. A download: P10.3. Re-encoded bitmaps: P7. A recording: P20. A pasted audio item: its name (P12).
- M3 then applies the transform, the same-content reuse across case variants, lowercasing and hash names. With present files A and B of different content, and `<B>` the hash of B:

  | Present | Desired name, bytes | Stored | Written |
  |---|---|---|---|
  | `photo.jpg` = A | `Photo.JPG`, A | `photo.jpg` | nothing |
  | `photo.jpg` = A | `Photo.JPG`, B | `photo-<B>.jpg` | B |
  | none | `Diagram.PNG`, A | `diagram.png` | A |
  | `paste-<A>.jpg` = A | `paste-<A>.jpg`, A (the same image pasted again) | `paste-<A>.jpg` | nothing |
  | `paste-<H>.jpg` = A, H not A's hash (put there by something else) | `paste-<H>.jpg`, B with hash H | `paste-<H>-<H>.jpg` | B |

- **Size warning (R856).** Before a file is added, if its size is 0 bytes or more than 104,857,600 bytes (100 MiB), Meguri asks first (P21): such a file never syncs (`media/media-files` M7.1). Add Anyway adds the file and the item goes on as usual. Decided 2026-10-27: empty files warn too, since they never sync either. Each such file of a multi-item action asks on its own, in order.
- **Cancel** skips only that file: it is not added, and its item takes the fallback of a failed fetch (P10.2). A URL item, or a P13 URL token, becomes its link (P10.1); a P14.2 `src` stays as it was; a `data:` token stays as escaped text; image data (P11), an audio item (P12), an attached file, a photo or a recording yields nothing. The other items of the action go on, files already added stay, and P9 does not move on to a later step because of a Cancel. When nothing is left to insert, the field is unchanged and nothing is an input (P16). Decided 2026-10-27: Cancel means "do not add this file", not "undo the paste".
- **Written at once.** The file is added when the action happens, before the note is saved or added; it is not part of the note's undo step. Undoing the insertion (field history) or discarding the note leaves the file, which Check Media later reports as unused (`media/media-files` M13).
- **Refusals.** M3's `invalid_name` (the transform empties the name) and `ignored_name`, and any read or write failure, show one alert naming the file and its cause; that file adds nothing and inserts nothing. Other items of the same action go on.

## P5. Attach (R849, R252, R855)

- Attach (F3 or ⌥⌘A, R252; context `editor.richText`, `editor/formatting` F14) is enabled only while a rich-text field has focus. Sources: Files on every platform; Photo Library and Camera on iPhone and iPad (P6). On macOS Attach opens the file importer at once. On iPhone and iPad the toolbar's Attach control (`editor.toolbar.attach`) is a menu of the three sources; the Format menu item and the keys show the same three choices, anchored to that control. Camera is left out on a device without one. Labels: P21.
- The field that gets the snippet is the one focused when Attach started, by P25's target rule; presenting the importer, picker or camera may take focus from the field.
- Files: a system file importer that allows any file type and one file. Decided 2026-10-27: one file, as Anki. The file keeps its own bytes; its snippet follows P3, so `notes.pdf` is inserted as the bare name `notes.pdf`, and a HEIC file chosen from Files is stored and inserted as a bare `.heic` name (only P6 converts photos).
- The snippet replaces the field's selection at its caret, as one field-history entry, and is an input of the field (`editor/field-html` E1).
- Cancelling the importer, the photo picker, the camera or the recorder changes nothing: no file, no field change, no input (R855).

## P6. Camera and photo library (R852)

A Meguri addition (Anki desktop has neither; AnkiMobile has a similar size limit).

- Photo Library: the system photo picker, images only, any number of images; each becomes its own `<img>` snippet, in the order picked. Camera: the system camera, one still photo. A Live Photo gives its still image. Videos are not offered.
- Each image is decoded with its orientation applied, so the stored pixels are the right way up.
- **Image max size.** A device-local setting (not in the collection; U160 places it on Settings › Editing): Off, or the longest side in pixels, one of 640, 800, 1024, 1280, 1600, 2048 and 4096. Default 1024. When the image's longer side exceeds the setting, the image is scaled so its longer side equals the setting exactly and the other side is the scaled length rounded to the nearest integer, halves away from zero, at least 1. Smaller images and Off keep their size; nothing is made larger. Example: 4032×3024 at 1024 → 1024×768; 3024×4032 → 768×1024; 1000×500 → unchanged.
- Then the image is encoded and named by P7: JPEG `.jpg`, or PNG `.png` when `pasteImagesAsPng` is `true`. Never HEIC.
- Decided 2026-10-27: these images are named `paste-<hash>.<ext>` like pasted bitmaps, so the photo's own file name (`IMG_1234.HEIC`) is never used.

## P7. Encoding bitmaps (R868, `editor/config-keys` K2)

Applies to every image that Meguri decodes and writes again: P6's photos and P11's pasted or dropped image data. Files, downloads, pasted audio and `data:` images keep their own bytes.

| `pasteImagesAsPng` when encoding | Format | Settings | Extension |
|---|---|---|---|
| `false` (default, also when missing or unreadable) | JPEG | lossy quality 0.8 (Anki's 80 of 100), baseline | `jpg` |
| `true` | PNG | the system encoder's defaults, not interlaced | `png` |

- An image with an alpha channel encoded as JPEG is first drawn over opaque white. PNG keeps alpha.
- Only the first frame of an animated image is encoded.
- The encoded file carries the source's color profile and no other metadata: no EXIF, GPS, TIFF, maker note or orientation tag.
- **Name.** `paste-` + the content hash of the encoded bytes + `.` + the extension, for example `paste-0beec7b5ea3f0fdbc95d0dd47f3c5bc275da8a33.jpg`. That is the desired name of the add (P4); M3 keeps it, since it is already lowercase.
- The bytes come from Apple's encoder, not Qt's, so the same picture gets other bytes, and so another name, than in Anki. Goldens are compared on the name's form and on the stored file's hash matching its name, never on bytes (P23).

## P8. Choosing the mode (R866, R139, `editor/config-keys` K3)

- A paste is in keep mode when `pasteStripsFormatting` reads `false`, in strip mode when it reads `true`; Shift held inverts it. So ⌘V uses the setting; ⇧⌘V and ⌥⇧⌘V use the other mode. The key is read at each paste.
- **Commands.** Decided 2026-11-10. Two commands, bound once with U158's registry in context `editor.richText` (`editor/formatting` F14):

  | Command | Label when `pasteStripsFormatting` is `false` | Label when `true` | Key shown in the menu | Other keys | Mode |
  |---|---|---|---|---|---|
  | Paste | Paste | Paste | ⌘V | | the setting's |
  | Other-mode paste | Paste and Match Style | Paste With Formatting | ⌥⇧⌘V | ⇧⌘V | the other one |

  On macOS both are Edit menu items; the second takes the place of the system's Paste and Match Style item, and its label is read each time the menu is shown, so it always names what it does (as P19's label). On iPad the same two commands are in the Edit menu of the menu system with the same keys; ⇧⌘V is a key command with no menu entry, so the ⌘ overlay shows ⌥⇧⌘V only (F14's rule for other keys). On iPhone and iPad the edit menu of a field (long press, secondary click) is P19's menu. The context menu's second paste item (P19) runs the other-mode command.
- A drop uses the setting, inverted when Shift is held at the moment of the drop (P17). On macOS Shift is read from the drop's own modifier flags. On iPhone and iPad it is the hardware keyboard's Shift state as the system reports it when the drop is performed (the Game Controller framework's `GCKeyboard`); with no hardware keyboard, or when that state cannot be read, Shift counts as not held and the setting's mode applies. Decided 2026-11-10.
- The mode changes only P13 (text tokens) and P15 (filter of pasted HTML). File, URL, image and audio items (P10 to P12) are handled the same in both modes.
- Paste into a field's HTML source editor (⇧⌘X, U190) is not this pipeline: it inserts the pasteboard's plain text verbatim. With no field focused, paste does nothing.

## P9. Reading the pasteboard (R867)

The first step that yields non-empty markup wins; later steps are not tried. Step 2 is the exception below: it wins as soon as its type is present. Step 3 has two orders.

1. **Internal content.** The pasteboard holds Meguri's private field-content type (written by Meguri's copy and cut in a field, and by Copy Image, P19): its HTML is used in internal mode. Meguri writes that type with every copy from a field, holding the copied HTML in storage form (editor-only attributes removed, names decoded as at save), so nothing in it needs rewriting. Any later copy elsewhere replaces the whole pasteboard and so removes it. Decided 2026-10-27: the type's identifier is `net.meguri.field-html`, the same in every build variant, declared by the app as an exported type conforming to `public.data`; its value is the copied fragment as UTF-8 HTML with no byte order mark. U194 defines the constant (it writes the type first, P19), and U197 reads it.
2. **HTML.** The first present of: `public.html`; the main resource of a web archive (`com.apple.webarchive`) when its type is `text/html`; RTF or RTFD (`public.rtf`, `com.apple.flat-rtfd`) converted to HTML by the system's attributed-string HTML export. Its result is external HTML: P14 then P15 in the paste's mode. Decided 2026-10-27: RTF counts as HTML, as for Anki on macOS, where its toolkit offers RTF content as HTML; there is no golden for it (P1). A present HTML representation ends the search, as in Anki, even when it is empty or P14 and P15 leave nothing of it; P16's empty-result rule then applies, and the later steps are not tried. So `public.html` `<style>p{}</style>` with the text `hello` pastes nothing. Decided 2026-11-10, the text of each source: `public.html` bytes are read as UTF-8 (a leading byte order mark removed, ill-formed sequences as U+FFFD), and a `<meta charset>` inside is ignored; a web archive's main resource is read in its own text encoding (its IANA name; UTF-8 when absent or unknown); RTF and RTFD are read by the system into an attributed string and written as HTML by P9.1, not by the system's HTML export, which puts the formatting into a style sheet and class names that P14.1 and P15 remove.
3. Then, when the pasteboard's first URL is a `file://` URL: **URLs** (P10), **image** (P11), **audio** (P12). Otherwise: **image**, **audio**, **URLs**.
4. **Text** (P13).
5. Nothing yields markup: the paste does nothing.

The result of steps 3 and 4 is inserted in internal mode: P15.3 only, so links, `<br>` and `&nbsp;` made there survive strip mode.

Pasteboard kinds, by platform:

| Kind | macOS (`NSPasteboard`, drag pasteboard) | iOS and iPadOS (`UIPasteboard`, drop item providers) |
|---|---|---|
| URLs | the items' `public.file-url` and `public.url` values, in item order | the same types, plus **file items**: an item provider that offers a file representation of a type with a media extension (P2), or of a type that is neither image nor audio nor video, counts as a `file://` URL to that file, named by its suggested name (the type's preferred extension appended when the name has none) |
| image | the first item with image data the system can decode, preferring `public.png`, then `public.tiff`, then `public.jpeg`, then any other image type | the same; an image item whose type has no media extension (HEIC, TIFF, BMP) is image data, so P7 converts it |
| audio | a file promise item (`NSFilePromiseReceiver`) whose promised file name has an audio or video extension (P2): the file is received into a temporary folder and its name is the received file's name. Audio or video data that comes with no file name (a bare `public.mp3` representation) is not an audio item | an item provider offering a type that conforms to `public.audiovisual-content`, whose `suggestedName`, with that type's preferred extension appended when it has none, has an audio or video extension (P2); the bytes are that type's file representation. Such an item is also a file item of the URLs row; whichever of the two steps comes first in step 3 takes it, with the same result |
| text | `public.utf8-plain-text`, else `public.plain-text` | the same |

**Item selection.** Decided 2026-11-10. HTML (step 2): the first item, in item order, that has one of step 2's types, and within it step 2's order. Image: the first item that has image data the system can decode, and within it the preference above, then its other image types in the item's own type order. Audio: the first item that qualifies by the audio row. Text: the first item that has `public.utf8-plain-text` or `public.plain-text`, and within it the former. URLs: every item's, in item order. Example: item 1 offers `public.tiff`, item 2 offers `public.png` and `public.utf8-plain-text`: the image step takes item 1's TIFF.

Decided 2026-10-27: on iPhone and iPad a file item without a media extension (a PDF from Files) is added (P4) and gets its bare-name snippet, as Attach does, instead of P10.1's link: a link to an app's temporary file path would be useless in the field. On macOS P10.1 applies, as in Anki.

Examples: HTML and text → the HTML. Text plus a `file://` URL list → the files. An image plus an `https` URL → the image. A Finder copy (file URL, its icon as TIFF, its name as text) → the file. An address-bar copy (`public.url` and text) → a link, in both modes.

### P9.1 RTF to HTML

Decided 2026-11-10. Meguri's converter writes the attributed string of P9 step 2's RTF or RTFD as HTML. Its result is external HTML, so P14 and P15 then run in the paste's mode: strip mode keeps only the `div` and `br` elements.

| In the attributed string | HTML |
|---|---|
| a paragraph (text up to LF, CR LF, CR or U+2029) | `<div>…</div>`; an empty paragraph is `<div><br></div>` |
| U+2028 | `<br>` |
| text | `&`, `<` and `>` escaped; spaces, TAB and U+00A0 as they are |
| bold trait | `<b>` |
| italic trait | `<i>` |
| any underline style | `<u>` |
| foreground color, converted to sRGB, other than black | `color: rgb(R, G, B);` |
| background color, converted to sRGB | `background-color: rgb(R, G, B);` |
| superscript, or a positive baseline offset; subscript, or a negative one | `<sup>`; `<sub>` |
| link | `<a href="URL">`, `href` escaped as P10.1 says |
| attachments, fonts, sizes, alignment, lists, strikethrough, shadows | nothing |

- R, G and B are 0 to 255, rounded half away from zero; alpha is ignored. Colors go into one `<span style="…">`, `color` first.
- Nesting, outermost first: `a`, `span`, `b`, `i`, `u`, `sup` or `sub`. Adjacent runs with the same attributes are one run, so an element never closes and reopens between them.
- Example: a TextEdit line "Meet **today** at noon" with "noon" in pure red: `<div>Meet <b>today</b> at <span style="color: rgb(255, 0, 0);">noon</span></div>` in keep mode, `<div>Meet today at noon</div>` in strip mode.

## P10. URL items (R867, R870)

### P10.1 Each URL

Each URL is taken up to its first line break; an empty URL is skipped. The snippets and links of all URLs are joined with nothing between them.

- **Media URL**: the URL text, lowercased, ends with `.` and a media extension (P2). The test is on the whole text, query included, so `https://ex.com/a.mp3?x=1` is not a media URL, while `https://ex.com/A.MP3` and `file:///Users/me/b%20c.JPG` are. Both Anki editors test this way. The plan's U197 test scenario that downloads `a.mp3` from `https://ex.com/a.mp3?x=1` pasted as text is superseded by this rule: that text becomes a link (P13). R870's "query string dropped" is about naming (P10.3), not about this test.
- A media URL is fetched (P10.2). On success the bytes are added (P4) with the desired name of P10.3, and the item becomes the snippet of the stored name (P3). On failure, or when it is not a media URL, it becomes a link.
- **Link**: `<a href="URL">TEXT</a>`. TEXT is the URL percent-decoded (P2), then `&`, `<`, `>`, `"` and `'` escaped. In the `href` value Meguri escapes `&` and `"`. Example: `https://ex.com/a%20b?x=1&y=2` → `<a href="https://ex.com/a%20b?x=1&amp;y=2">https://ex.com/a b?x=1&amp;y=2</a>`.
- A `file://` URL to a file without a media extension (a PDF, a HEIC photo, a folder) becomes the link above, with the file URL as `href` and its decoded form as TEXT; nothing is copied: `file:///Users/me/notes%20v2.pdf` → `<a href="file:///Users/me/notes%20v2.pdf">file:///Users/me/notes v2.pdf</a>`. Decided 2026-10-27: as Anki, on macOS; iPhone and iPad file items follow P9's decision instead.

### P10.2 Fetching

- **One implementation.** Decided 2026-11-10: the fetch rules of this section and P10.3's name are a service of the Rust core. `RemoteMediaFetcher` calls it through the FFI and adds the busy state, Cancel and alerts below; the conformance executor calls the same service (`specs/conformance/editor-ops.md` E4), so its goldens test the code the app runs.
- `file://` URLs: the URL's path, without query and fragment (an unencoded `?` or `#` starts them), is percent-decoded to bytes. Bytes that are not UTF-8 fail with no disk lookup; otherwise the file at that path is read. An encoded `%23` is part of the name: `…/a%23b.png` reads `a#b.png`. A file the app cannot read (sandbox, missing) is a failure.
- `http://` and `https://` URLs: the URL is parsed as the WHATWG URL standard does (dot segments removed, spaces and non-ASCII characters percent-encoded), and that URL is requested with one GET, with User-Agent `Mozilla/5.0 (compatible; Meguri)` and `Accept-Encoding: gzip, deflate`; a body sent with `Content-Encoding` `gzip` or `deflate` is stored decoded. Redirects (301, 302, 303, 307 and 308 with a `Location`) are followed, up to 10; an 11th redirect, or a redirect to a scheme other than `http` or `https`, is a failure. The fetch fails when no data arrives for 30 seconds, on a network error, or when the final status is not 200 (so also on a 3xx that is not followed). Decided 2026-10-27: Meguri does not name Anki in its User-Agent. Decided 2026-11-10: the redirect limit, the schemes, the parsing and the compression, as Anki's page editor and its default editor do between them.
- `ftp://` URLs always fail, as in Anki, whose fetch has no FTP support either: the failure alert below, then the fallback.
- Fetches run one after another, in pasteboard or document order; the insertion happens when every fetch of the paste or drop has ended.
- **Busy state.** Decided 2026-11-10. While fetches run, the target field is busy (P25 `setBusy`): it takes no input. The editor shows a native indeterminate progress indicator with the text "Getting Media…" and a Cancel button; its placement is the screen contract's (`docs/design`). Cancel stops the running fetch and every remaining one of that paste or drop: each stopped item takes its fallback with no alert, files already added stay, and the insertion then happens with what is ready. While a field is busy, paste and drop do nothing anywhere in that editor and their commands are disabled; typing and other commands in other fields go on. Loading another note or closing the editor cancels as Cancel does, and P25's target rule then inserts nothing.
- Each failure shows one alert (P21) naming the URL and its cause, then the paste goes on with the failed item's fallback (a link, or P14's unchanged `src`).
- No limit on size beyond P4's warning, which follows the fetch.

### P10.3 Name of a fetched file

The name comes from the URL as written, before P10.2's parsing and before any redirect; the media type of step 4 comes from the final response. So `https://ex.com/moved`, redirected to an `image/png` response, is `moved.png`.

1. Take the URL without its query (from the first `?`) and its fragment (from the first `#`).
2. Percent-decode it (P2), then take what follows its last `/`. Decoding comes first, as in Anki, so an encoded slash splits the name: `https://ex.com/dir/a%2Fb.png` gives `b.png`.
3. If that is empty or only text whitespace (P2), use `paste`.
4. For an `http` or `https` fetch whose name has no extension (no `.` after its leading dots), append the extension for the response's media type: the `Content-Type` header up to its first `;`, trimmed and ASCII-lowercased:

   | Media type | Appended |
   |---|---|
   | `audio/mpeg` | `.mp3` |
   | `audio/ogg` | `.oga` |
   | `audio/opus` | `.opus` |
   | `audio/wav` | `.wav` |
   | `audio/webm` | `.weba` |
   | `audio/aac` | `.aac` |
   | `image/jpeg` | `.jpg` |
   | `image/png` | `.png` |
   | `image/svg+xml` | `.svg` |
   | `image/webp` | `.webp` |
   | `image/avif` | `.avif` |
   | anything else | nothing |

Examples: `https://ex.com/dir/Pic.PNG?size=2` → `Pic.PNG` (M3 stores `pic.png`); `https://ex.com/img?id=3` with `image/png` → `img.png`; `https://ex.com/` with `image/jpeg; charset=binary` → `paste.jpg`; `https://ex.com/a.png#top` → `a.png`; `https://ex.com/.hidden` with `image/png` → `.hidden.png`.

`.weba` is not a media extension, so a file named that way gets the bare-name snippet (P3, R849). Such a name reaches P10.1 when a media URL's path has no extension, as in `https://ex.com/play?f=a.mp3` answered with `audio/webm` (`play.weba`) or with an unlisted type (`play`). Anki's default editor writes `[sound:play.weba]` for it and plays it, since it writes a sound tag for every name without an image extension (P23).

## P11. Image data (R868)

The image is decoded, encoded by P7 (`pasteImagesAsPng` read at this paste), added (P4) and becomes its `<img>` snippet. No size limit applies (P6's applies only to photos). Decided 2026-11-10: when the system cannot decode the image data, the image step yields nothing and P9 tries the next step; no file is added (Anki adds and inserts an empty `paste-da39a3ee5e6b4b0d3255bfef95601890afd80709.jpg`, `editor/paste-sources` S8; P23). Example: a PNG screenshot pasted with the default setting → `<img src="paste-<40 hex>.jpg">`, the hex being the SHA-1 of the stored file.

## P12. Audio item (R867)

The item's bytes are added under its name (P9's audio row: the received file's name on macOS, the suggested name on iPhone and iPad) and become its `[sound:…]` snippet, playing once (P3). Without such a name with an audio or video extension the step yields nothing.

## P13. Plain text (R869, R870)

The text is split into lines at each LF (U+000A); CR is not a line break and stays in the converted text, so CRLF text gives a CR before each `<br>`. P15.3's HTML parse then turns each CR into LF, as `editor/field-html` H3's table shows, so the field holds `a` LF `<br>b` for `a` CR LF `b`. Each line is split into alternating runs of text-whitespace (P2) and other characters. Lines are joined with `<br>`; nothing follows the last line, so a text ending in LF ends with `<br>`.

- **Whitespace run and ordinary token.** Escape `&`, `<`, `>`, `"` and `'`, then turn each TAB into four spaces, then rewrite each run of two or more U+0020 spaces as (run length − 1) × `&nbsp;` followed by one space. A single space stays a space; other whitespace characters (U+00A0, U+3000, CR) stay as they are. The no-break spaces come first and the plain space last, as in Anki; R869's "every space after the first in a run" gives the count (n − 1 of n spaces), not the order.
- **Keep mode only.** A non-whitespace token that starts with `data:image/` becomes the snippet of P14.2 (or stays text, P14.2). A token that starts with `http://`, `https://`, `ftp://` or `file://` (ASCII case-insensitive) is a URL and becomes what P10.1 makes of it: a snippet or a link.
- In strip mode every token is escaped text.
- The result then goes through P15.3, whose trim (P2) removes whitespace at both ends.

| Text (`\t` TAB, `\n` LF) | Mode | Field gets |
|---|---|---|
| `a\tb   c\nd<e>` | either | `a&nbsp;&nbsp;&nbsp; b&nbsp;&nbsp; c<br>d&lt;e&gt;` |
| ` x ` | either | `x` |
| `  x` | either | `&nbsp; x` |
| `a\n\nb\n` | either | `a<br><br>b<br>` |
| `say "hi" & 'bye'` | either | `say "hi" &amp; 'bye'` (the page's serialization writes quotes back as characters) |
| `see https://ex.com/page now` | keep | `see <a href="https://ex.com/page">https://ex.com/page</a> now` |
| `see https://ex.com/page now` | strip | `see https://ex.com/page now` |
| `https://ex.com/a.mp3` | keep | `[sound:a.mp3]` after a successful fetch |

The plan's U197 example writes the TAB as four plain spaces; by this rule it is `&nbsp;&nbsp;&nbsp; `. The goldens of P24 `text-plain` decide the exact layout. The plan's example `https://ex.com/a.mp3?x=1` → `[sound:a.mp3]` is superseded too: that token is not a media URL (P10.1), so keep mode makes it a link.

## P14. Pre-pass on external HTML (R870, R871)

External HTML (P9 step 2) is parsed as an HTML fragment into an inert document (no scripts run, nothing loads); P14 and P15 work on that tree, and it is serialized once at the end. Decided 2026-11-10, the split between host and page (P25): the host sends the HTML with `pasteScan`, and the page parses it, applies P14.1 and lists the `img` sources; the host does P14.2's fetches and decodes; then `pasteFilter` parses the same text again with the same parser, so it builds the same tree, applies P14.1, sets the `src` values the host rewrote, runs P15 in the page's CSS object model (P15.4) and returns the serialized result, which the host inserts with `insert`.

### P14.1 Elements

In both modes: `script`, `iframe`, `object` and `style` elements are removed with their content, and each `p` element becomes a `div` (P15 then drops its attributes).

### P14.2 Image sources

For each `img` with a `src`, in document order:

- `src` starting with `http://`, `https://`, `ftp://` or `file://` (ASCII case-insensitive): fetched as P10.2, whatever its extension. On success, added with the name of P10.3 and `src` becomes the stored name. On failure `src` is left as it was.
- `src` starting exactly with `data:image/jpg;base64,`, `data:image/jpeg;base64,`, `data:image/png;base64,` or `data:image/gif;base64,`: the rest, with ASCII whitespace removed, is decoded as standard base64 with padding. The decoded bytes are added as `paste-<their hash>.<ext>`, `ext` being `jpg` (for both `jpg` and `jpeg`), `png` or `gif`; the bytes are not re-encoded. `src` becomes the stored name.
- Decided 2026-10-27: a `data:image/…` source of another type, or one that does not decode, is left unchanged; a P13 token of that kind stays as escaped text.
- Any other `src` (a relative name, `about:`, `blob:`) is left unchanged.

This runs in both modes, so strip mode also brings remote and inline images into media. Decided 2026-11-10: R870's keep-mode condition applies to P13's text tokens only; this pre-pass (R871, which has no mode condition) and P10's URL items run in both modes, as in both Anki editors. Example, strip mode: `<p>see <img alt="d" src="data:image/png;base64,aW5saW5lIGdpZiBieXRlcw=="></p>` adds `paste-bde96ed1fbd958a421a14ce5cedd73191801c63d.png` and inserts `<div>see <img alt="d" src="paste-bde96ed1fbd958a421a14ce5cedd73191801c63d.png"></div>`.

## P15. Filters (R872 to R875)

The filter works on the tree after its children are done: an element's children are filtered before the element itself. Comments are removed in every mode. Tag names are matched in the HTML namespace (an `svg` or MathML element is never on a list); attribute names are matched ASCII case-insensitively.

### P15.1 Strip mode (R873)

| Element | Result |
|---|---|
| `br`, `div`, `sub`, `sup` | kept, all attributes removed |
| `img` | kept with only `src` and `alt` |
| `p` | replaced by a `div` with no attributes holding the same children |
| `title`, `script`, `iframe`, `object`, `style` | removed with their content |
| any other element with child nodes | replaced by its children (unwrapped) |
| any other element without child nodes (left) | removed |

Then, on the serialized HTML text: every run of LF, TAB and space (U+000A, U+0009, U+0020 only) becomes one space, and the result is trimmed (P2). This also applies inside attribute values. `&nbsp;` is serialized as that entity, so it survives both.

### P15.2 Keep mode (R874, R875)

The strip-mode table, plus:

| Element | Kept attributes |
|---|---|
| `b`, `blockquote`, `code`, `dd`, `dl`, `dt`, `em`, `h1`, `h2`, `h3`, `i`, `li`, `ol`, `pre`, `rp`, `rt`, `ruby`, `strong`, `table`, `u`, `ul` | none |
| `a` | `href` |
| `font` | `color` in light appearance; none in dark appearance |
| `td`, `th` | `colspan`, `rowspan` |
| `tr` | `rowspan` |
| `span` | `style`, filtered below |

- **`span` style.** Every other attribute is removed. Within `style`, each declared property (shorthands are first expanded into their longhands, as the CSS object model does) is kept only if listed: light appearance keeps `color`, `background-color` unless its serialized value is `transparent` (`editor/paste-filter` X5.1), `font-weight`, `font-style` and `text-decoration-line`; dark appearance (the editor's effective appearance at paste time) keeps only `font-weight`, `font-style` and `text-decoration-line` (R875). A `style` left with no property stays as `style=""`. Example, light: `<span class="x" style="font-size: 12px; font-weight: bold; color: #000">` → `<span style="font-weight: bold; color: rgb(0, 0, 0);">`; dark: `<span style="font-weight: bold;">`. A shorthand keeps the listed longhands it expands to, so `font: bold 12px Arial` leaves `font-style` and `font-weight` (the golden records their order).
- Decided 2026-10-27: in dark appearance `font` loses its `color` too, as R875 asks (keep mode drops color in dark appearance so pasted text stays readable). Anki keeps `<font color>` in night mode (P23). Example, dark: `<font color="#000" face="Arial">x</font>` → `<font>x</font>`.
- Elements not listed (`h4` to `h6`, `tbody`, `thead`, `section`, `hr`) follow the strip table's last two rows.
- After the tree is serialized, the text is trimmed (P2); whitespace runs are kept.
- Decided 2026-10-27: an `href` whose value, after removing leading ASCII whitespace and control characters, starts with `javascript:` (ASCII case-insensitive) is removed and the `a` stays without it, so a page's script link never reaches a card (P23).

### P15.3 Internal mode (R872)

Elements and attributes are all kept. For each HTML element with a `style`, the properties `font-size`, `font-family`, `width`, `height`, `max-width` and `max-height` are removed, and `vertical-align` is removed on `img` elements only; an emptied `style` stays as `style=""`. The serialized text is trimmed (P2).

Example: copying `<span style="font-size:30px;color:red">x</span>` from one field and pasting it into another inserts `<span style="color: red;">x</span>`.

### P15.4 Style serialization

Style values are written as the CSS object model serializes them (`name: value;` pairs joined by one space, colors as `rgb(…)`). WebKit and Chromium can serialize a value differently; the conformance harness compares `style` values as ordered lists of property and value pairs after parsing both sides with the page's CSS parser, and the rest of the HTML byte for byte (P24).

## P16. Insertion and what the field holds

- An empty filter result inserts nothing: the field is unchanged and nothing is an input.
- P3's src encoding is applied, once, by the host, to each `img` `src` that holds a name Meguri itself wrote: a P3 snippet (P10 to P13), a P14.2 rewrite to a stored name, and every local (`text/media-references` M5) `src` of internal content (P9 step 1, P17's drags from a field), which holds storage-form names. The host builds P3 snippets already encoded, and passes the other two as `pasteFilter`'s rewrites (P25), so the encoding happens before the filter: strip mode's whitespace collapse (P15.1) never meets a raw stored name, and a fetch stored as `a  b.png` is inserted as `src="a%20%20b.png"`. The save decodes it again.
- Any other `src` of external HTML (a relative name the source wrote, such as `x%20y.png`, or one P14.2 left unchanged) is inserted as parsed, with no encoding. The save then decodes it like any name (M7), so `<img src="x%20y.png">` is stored as `<img src="x y.png">`, as in Anki.
- A non-empty result replaces the focused field's selection at its caret as one field-history entry, and is an input of the field (`editor/field-html` E1). The host sends it with P25's `insert`, under P25's target rule, since fetches and alerts can outlast the field's focus. Meguri's page inserts the parsed nodes itself, never through WebKit's own paste or insert commands, and must produce the field HTML the goldens record, starting from an empty field and from a field with text and the caret at its end.
- The note is saved by U190's pipeline (`editor/field-html` H3, S1): snippets as P3 says, names raw (decoded by `text/media-references` M7), `data-meguri-editor` removed, `<br>` or `<div><br></div>` alone stored as empty.
- Media files were already written (P4). The note's media references are whatever the stored field names (`text/media-references` M4).

## P17. Drop (R876, R877)

- Meguri first finds the field under the drop point, by asking the page (P25 `fieldAt`). Not a field, a collapsed field, or a field whose note type hides it (image occlusion, U206; the page has no such field): the drop does nothing, and no file is added. Decided 2026-10-27: the target is checked before any item is processed (P23).
- That field is focused. If only its HTML source editor is showing, its rich-text view is shown first. The content goes at the end of the field, as in Anki, which puts the caret there.
- **Drags from a field.** Decided 2026-11-10. When a drag starts from a selection in a rich field, the page sends `fieldDrag` (P25) with the selection's HTML in storage form, as P9 step 1's copy writes it, and the app keeps it as the current field drag until that drag session ends. The private type of P9 step 1 is not written to the drag pasteboard, which the web view builds. A drop that belongs to the current field drag's session (macOS: the drag's source is a Meguri editor web view; iPhone and iPad: the drop session has a local drag session) inserts the kept HTML in internal mode (P15.3), with no pre-pass (R877), into a field of this editor or of another one, and ignores the drag pasteboard.
- **Copy, not move.** Decided 2026-11-10. A drag from a field copies, as in Anki's default editor, whose host takes every drop from its web view: the source field keeps its content. The page cancels the web view's own drop handling, so WebKit never moves the selection. A drop into the same field also copies, at the end of the field. The insertion is one field-history entry of the target field and an input of it; the source field is not changed and gets no entry.
- Anything else goes through P9 from step 2 with the mode of P8 (Shift held at drop inverts it), then P14 to P16. On macOS this covers Finder files, browser images, text and links; on iPhone and iPad, drags from other apps.

## P18. Images in the editor (R878, R879, R880)

These apply to every `img` element in a rich-text field (the editor typesets no MathJax, `editor/field-html` H1 step 4).

- **Shrink.** When `shrinkEditorImages` is `true` (default when missing, R848), each image is shown no larger than 250 × 125 px, keeping its aspect ratio, ignoring its own `width` and `height` for display. Double-clicking an image, or its Actual Size button, toggles that one image between shrunk and its own size. The toggle is kept in the image's `data-meguri-editor` attribute, which the save removes (`editor/field-html` H3 step 2), so it is never stored and is not an input. The image's **size** is W×H: W is its `width` attribute, or else its natural width; H is its `height` attribute, else, when it has a `width` attribute, W ÷ r truncated to an integer (r is natural width ÷ natural height), else its natural height. Double-click and the button do nothing when the size is at most 250 × 125; such an image is shown at its own size even while shrink is on.
- **The option and the toggle.** An image that has never been toggled follows `shrinkEditorImages`: shrunk while it is `true`, at its own size while it is `false`. Once toggled, the image's own state wins over the option in both directions, as in Anki: with Shrink Images off, double-clicking an image larger than 250 × 125 shrinks it, and an image toggled to shrunk stays shrunk when the option is turned off (an image toggled to its own size stays so when it is turned on). The toggle lasts until the note is loaded again. The label and the Actual Size control follow the image's effective state.
- **Selection.** Clicking an image selects it and shows the handle, the label and the controls. The label shows `(double-click to expand)` (iPhone and iPad: `(double-tap to expand)`) while the image is shrunk and can grow; otherwise `W×H`, the size above, followed by ` (Original: w×h)` (natural size) when the image has a `width` or `height` attribute.
- **Resize (R879).** The handle resizes only while the image is shown at its own size: while it is shrunk, dragging the handle does nothing. Decided 2026-10-27: an image whose size is at most 250 × 125 counts as shown at its own size, so its handle works with shrink on; in Anki its handle stays inactive while shrink is on, and double-click cannot toggle it (P23). Dragging keeps the aspect ratio r. The minimum width is 5 × r and the minimum height 5 when r is above 1; otherwise the minimum width is 5 and the minimum height 5 ÷ r. When the drag grew the width proportionally more than the height (dragged width ÷ natural width above dragged height ÷ natural height), the width is the larger of the dragged width truncated to an integer and the minimum width. Otherwise the height is the larger of the dragged height truncated to an integer and the minimum height, and the width is natural width × that height ÷ natural height. The `width` attribute written is the integer part of that width: with r = 2.5 a drag to 3 px wide writes 12, not 13. Each change writes the `width` attribute as an integer and removes `width` and `height` from the image's `style` (a `style` left empty is removed); a `height` attribute stays. Restore Original Size removes `width` and is disabled without one.
- **Float (R880).** Float Left and Float Right set `float: left` or `float: right` in the image's `style`; No Float removes `float` (a `style` left empty is removed). The control showing the image's current float is selected.
- Each resize gesture, restore and float change is one field-history entry and an input of the field (`editor/field-html` E1).

## P19. Field context menu (R881)

| Item | Shown | Action |
|---|---|---|
| Cut, Copy | text or an image is selected | the system action; writes the private type of P9 step 1 too |
| Paste | always | P8 with the setting's mode |
| Paste Without Formatting, or Paste With Formatting when `pasteStripsFormatting` is `true` | always | P8 with the other mode (decided 2026-10-27: the label names what the item does) |
| Copy Image | on an image the page has loaded | writes the image's bytes with their type, and for a local media image the private type holding `<img src="NAME">`, so pasting it into a field reuses the file instead of encoding a copy. The host reads a local media image's bytes from its file; for a remote image it fetches the `src` once as P10.2 does, and a failure shows P10.2's alert and copies nothing |
| Open Image | on any image; enabled only on a local media image | Quick Look (`QLPreviewPanel` on macOS, `QLPreviewController` on iOS and iPadOS) |
| Show in Finder | macOS, on any image; enabled only on a local media image | reveals the file in Finder |

The page tells the host what the press is on with P25's `contextTarget` before the menu shows. A local media image is one whose `src` is not remote (`text/media-references` M5), whose raw name has an image extension (P2) and resolves to a file by `media/media-files` M5. Anki's menu has Cut and Copy only with a selection, Paste, and Copy image, Open image (in the default app) and Show in folder only on an image whose name has an image extension; Meguri's second paste item is an addition (R881).

Decided 2026-10-27: Copy Image is shown on any image the page has loaded, remote ones included, since copying loaded pixels needs no media file; Open Image and Show in Finder are shown on every image but disabled unless it is a local media image (`<img src="https://…">`, a missing file), so the menu keeps one shape (P23).

## P20. Recordings (R853, R854)

Record (F5 or ⌥⌘R; context `editor.richText`, `editor/formatting` F14; toolbar `editor.toolbar.record`) is enabled only while a rich-text field has focus, as Attach is: disabled in HTML mode, in the tag editor and with no field focused. It records AAC in an MPEG-4 container. Save adds the file with the desired name `rec` + the Unix time of the save in whole seconds (UTC) + `.m4a`, for example `rec1793102400.m4a`, and inserts its `[sound:…]` snippet at the caret of the field Record started in (P3, played once; P25's target rule). Anki records MP3 named the same way (`rec<seconds>.mp3`). Cancel adds nothing (R855).

The recorder is an in-window panel (R853; the screen contract owns its placement). Decided 2026-10-27, its states:

- **Permission.** The first Record asks for microphone access through the system prompt. When access is denied or restricted the panel shows P21's denied message with Open Settings (opens the app's page in Settings) and Cancel, and records nothing.
- **Recording.** Recording starts as soon as access is granted and the panel shows. The panel shows the elapsed time as `m:ss` (`h:mm:ss` from one hour on), a level meter, Save and Cancel. Save stops the recording and adds the take; Cancel stops it and discards the take. Esc is Cancel; Return is Save. There is no length limit; P4's size warning applies to the take at Save.
- **Stopped.** An interruption (a call, another app taking the audio session, an audio route loss such as an unplugged microphone) or the app moving to the background or resigning active stops the recording and keeps the take recorded so far. The panel then shows P21's stopped message, the final time, Save and Cancel; recording does not resume. Nothing is ever saved without Save. A take left unsaved when the app ends is a temporary file, removed at the next launch; it never reaches media.
- **Failure.** When the recorder cannot start or fails while recording, the panel closes with P21's alert and nothing is added.
- **Target.** The field keeps its selection while the panel is up (`editor/field-html` F4). When the field can no longer take the snippet at Save (another note loaded, the note closed, the field in HTML mode or collapsed), Save still adds the take, so the audio is not lost, and inserts nothing (P25); Check Media later reports the file as unused (`media/media-files` M13).

## P21. Strings

Meguri's own wording, for the String Catalog. Anki's English is given where an item matches one of its labels.

| Where | Meguri | Anki |
|---|---|---|
| Context menu | Cut · Copy · Paste · Paste Without Formatting · Paste With Formatting · Copy Image · Open Image · Show in Finder | Cut · Copy · Paste · Copy image · Open image · Show in folder |
| Image controls | Float Left · No Float · Float Right · Actual Size · Restore Original Size | Float left · No float · Float right · Toggle actual size · Restore original size |
| Image label | `(double-click to expand)` · `(double-tap to expand)` · `W×H` · ` (Original: w×h)` | `(double-click to expand)` · `W×H` · ` (Original: w×h)` |
| Size warning (P4) | title "This File Won't Sync"; message "“NAME” is SIZE. Files this size don't sync to your other devices."; buttons "Add Anyway" and "Cancel" | none |
| Edit menu (P8) | "Paste" · "Paste and Match Style" (setting `false`) · "Paste With Formatting" (setting `true`) | none |
| Fetch progress (P10.2) | "Getting Media…"; button "Cancel"; accessibility label of the indicator "Getting Media" | none |
| Fetch failure (P10.2) | title "Couldn't Get Media"; message "URL" then on its own line the cause: "The server answered with status N." or the system's error description | an alert naming the error, or giving the status code |
| Add failure (P4) | title "Couldn't Add “NAME”"; message the cause | none |
| Attach (P5) | toolbar control "Attach"; Format menu item "Attach…"; sources "Choose File…" · "Photo Library" · "Take Photo" | "Attach pictures/audio/video" |
| Record (P20) | toolbar control and Format menu item "Record Audio…"; panel title "Record Audio"; timer `m:ss` or `h:mm:ss`; buttons "Save" and "Cancel"; stopped message "Recording stopped."; denied: title "Microphone Access Is Off", message "To record audio, allow Meguri to use the microphone in Settings.", buttons "Open Settings" and "Cancel"; failure alert title "Couldn't Record Audio", message the cause; microphone usage description (`NSMicrophoneUsageDescription`) "Meguri records audio for your notes when you choose Record Audio." | "Record audio" |

SIZE is the system byte-count format (decimal units). The screen contract (`docs/design`) owns placement and appearance.

## P22. Decisions

Each is stated where it applies; listed here so a reviewer finds them. Decided 2026-10-27: P1 (oracle), P4 (empty files warn; Cancel skips one file), P5 (one file), P6 (photo naming), P9 (private type identifier; RTF as HTML; iOS file items), P20 (recorder states), P25 (target rule), P10.1 (non-media file URLs as links), P10.2 (User-Agent), P14.2 (unsupported data images left in place), P15.2 (`javascript:` links; no `font` color in dark appearance), P17 (target first, end of field), P18 (small images resize with shrink on), P19 (second paste item's label; image items on every image). Decided 2026-11-10: P2 (percent-decoding), P8 (menu commands and labels; Shift at a drop on iPhone and iPad), P9 (item selection; the text decoding of HTML sources), P9.1 (RTF converter), P10.2 (one core fetch service; redirects, schemes, parsing and compression; `file://` decoding; busy state and Cancel), P10.3 (name from the URL as written), P11 (undecodable image data yields nothing), P14 and P25 (the host and page split), P14.2 (both modes), P16 (encoding before the filter), P17 (drags from a field: kept HTML, copy, not move). The order of concerns: data safety, then the plan's requirements, then Anki compatibility, then simplicity.

## P23. Differences from Anki

Proposed `conformance/deviations.yaml` ids, to be entered with the goldens that show them:

| Situation | Anki 26.09.3 (default editor) | Meguri | Deviation |
|---|---|---|---|
| Bitmap re-encoding | Qt's encoder; JPEG drops alpha (transparent pixels usually turn black) | Apple's encoder; alpha drawn over white; other bytes, so another `paste-<hash>` name | `editor-paste-image-bytes` |
| Image data the system cannot decode | an empty `paste-<hash of no bytes>.jpg` is added and inserted | the image step yields nothing and P9 tries the next step (P11) | `editor-paste-undecodable-image` |
| URL with a fragment | the fragment stays in the fetched file's name | removed | `editor-paste-url-fragment` |
| `Content-Type` with parameters | no extension appended | the media type before `;` decides | `editor-paste-mime-params` |
| `data:image/webp;base64,…` or other types; base64 with inner whitespace or bad data | `src` emptied; a text token dropped; bad data fails the paste with an error | left unchanged; whitespace ignored | `editor-paste-data-url-kept` |
| Unclosed elements in external HTML (`<p>a<p>b`) | an extra parse with a non-HTML5 parser can nest them before the page parses | one HTML5 parse | `editor-paste-html-parser` |
| `href="javascript:…"` in keep mode | kept | removed | `editor-paste-script-href` |
| `"` in a link's URL | written into `href` unescaped | escaped | `editor-paste-href-escaping` |
| A stored name with `%` and two hex digits written into `src` by a P14.2 fetch (`a%2541.png` fetched as `a%41.png`) | written raw, so the save decodes it and the field names another file (`aA.png`) | encoded at insertion (P16), so the field names the stored file | `editor-paste-src-encoding` |
| `<font color>` in keep mode, dark appearance | color kept | color removed (R875) | `editor-paste-dark-font-color` |
| Media URL whose fetched name has no media extension (`play.weba`, `play`) | `[sound:NAME]`, played | the bare name (R849) | `editor-paste-bare-name` |
| Resize handle on an image of at most 250 × 125 with shrink on | inactive while shrink is on (double-click does nothing on it) | works (P18) | `editor-image-resize-small` |
| Image items of the context menu | shown only on an image whose name has an image extension | Copy Image on any loaded image; Open Image and Show in Finder on every image, disabled unless local media | none (no golden) |
| Drop outside a field or on a collapsed field | the items' media files are added anyway, nothing is inserted | nothing is added | `editor-drop-outside-field` |
| Attach of a non-media file | the default editor's picker offers media types only | any file, bare-name snippet (R849, the experimental editor's rule) | none (no golden) |
| Photo library, camera, size limit, RTF, iPhone and iPad file items | none on desktop Linux | P6, P9 | none (no golden) |

## P24. Scenarios

Requested from the oracle maintainer, in `conformance/scenarios/editor/paste/`, observed in the quarantined editor image (P1) with Anki's default editor; none is recorded yet and no runner step or oracle op exists for them. Each scenario sets: the pasteboard as MIME types and values (`text/html`, `text/uri-list`, `image/png` bytes, `text/plain`), Shift held or not, `pasteStripsFormatting`, `pasteImagesAsPng`, night mode on or off, the target field, its starting HTML and the files already in media. Observations: each field's stored HTML after the editor saves; the media folder (names, content hashes, sizes); for re-encoded images their pixel size and that each name's hex equals the file's SHA-1; the warnings shown. A local HTTP server inside the image serves fetched URLs; the internet is never used.

| Scenario | Covers |
|---|---|
| `order-html-wins` | P9: HTML with text, HTML with an image, HTML with file URLs; `text/html` `<style>p{}</style>` with `text/plain` `hello` (nothing is pasted) |
| `order-file-first` | P9: a `file://` list with an image and text (files win); the second URL remote |
| `order-image-first` | P9: image with an `https` media URL, image with text |
| `order-url-over-text` | P9: an `https` page URL with text, in both modes |
| `text-plain` | P13: the table's inputs, CRLF text, leading and trailing spaces, empty lines, TAB alone, `&"'<>`; U+00A0, U+3000 and U+0085 inside and at both ends of a line, U+FEFF at both ends; in keep mode a URL token followed by U+3000 |
| `text-keep-tokens` | P13: keep and strip mode with `https://…/a.mp3`, `https://…/a.mp3?x=1`, `HTTP://…/A.PNG`, `https://…/page`, `file://` media, a `data:image/png;base64,` token, a `data:image/webp;base64,` token, a 404 media URL |
| `image-encode` | P7, P11: a PNG bitmap with alpha, both values of `pasteImagesAsPng`; the same bitmap pasted twice |
| `strip-filter` | P14.1, P15.1: one HTML covering every table row, comments, empty and nested unknown elements, `svg`, attributes on kept elements, whitespace in text and attributes, leading and trailing whitespace |
| `keep-filter` | P15.2: the same HTML in keep mode, plus spans with shorthand and longhand styles, `background-color: transparent`, a style emptied by the filter, `font color`, table attributes, `a href` with `javascript:` |
| `keep-filter-dark` | P15.2: the span and `font` cases in night mode |
| `internal-paste` | P15.3: copy from a field holding styled spans, an image with `vertical-align` and `width`, and a `script`; paste into another field; copy, then change the clipboard elsewhere, then paste |
| `html-img-sources` | P14.2, P16: remote `img` with 200 and a named path, 200 with `image/png` and no extension, 404, a 31 s delay, an `ftp://` source; a fetched path `a%2541.png`; `data:image/jpeg`, `data:image/gif`, `data:image/webp`, invalid base64; a readable `file://` image; a relative `src="x%20y.png"` |
| `fetch-names` | P10.3: the examples, including `Content-Type` with parameters, a URL with a fragment and `https://ex.com/dir/a%2Fb.png` |
| `media-naming` | P4: file URLs `Photo.JPG` against a present `photo.jpg` with the same and with other bytes; `Diagram.PNG`; names with `&`, spaces and non-ASCII characters |
| `snippets` | P3, P10.3: through the editor's media insert for `.jpg`, `.mp3` and `.webm` names; a media URL `…/play?f=a.mp3` answered with `audio/webm` and with `application/octet-stream` (`editor-paste-bare-name`) |
| `drop` | P17: a file onto the second field (content at its end), onto a collapsed field, Shift-drop of rich HTML, HTML dragged from another field, a selection dragged within one field (Anki's default editor copies it) |
| `image-resize` | P18: drag to below 5 px for a wide image with r = 2.5 and a tall image, a drag on the width and on the height, restore, an image with a `height` attribute and one with only a `width` attribute (the label), an image with inline `style` width and height, a drag while shrunk, a drag on an image of at most 250 × 125 with shrink on |
| `image-float-shrink` | P18: each float, No Float with other styles present; double-click toggling with shrink on and off, including a double-click on a 1000×800 image with shrink off and turning the option off after toggling an image to shrunk (the label); the stored HTML after each |
| `corpus` | P9 to P16: each payload under `conformance/corpora/paste/` (Word, Google Docs, Safari, Chrome, Yomitan, plain text: U197's corpus) replayed in strip and keep mode, in light and dark appearance; observed: the stored HTML and the media folder. The payloads are captured once as an M2 user step (the plan's U197 execution note); until then this scenario has no input |

Meguri-only behavior (P4's size warning, its Cancel and Add Anyway, P6, P9's RTF and web archive rows, P10.2's timeouts on Apple platforms, P19's Quick Look and Finder items and its image items, P20's format) is tested in U194's and U197's own tests.

## P25. Bridge messages and the target field

Additions to `editor/field-html` B2 and B3 (U194's insertion and image messages; U197's `pasteScan`, `pasteFilter`, `fieldAt`, `setBusy` and `fieldDrag`), same rules (B1); `editor/formatting` F13 points here.

| Direction | Type | Members | Reply or when |
|---|---|---|---|
| host to page | `insert` | `loadId`, `ord`, `html`, `at` (`selection` or `end`) | `changed` (boolean). `html` is final: P3 snippets (attach, photos, recordings), or `pasteFilter`'s result, with P16's src encoding already applied by the host; the page inserts it as `editor/formatting` F10 says and adds nothing. `selection` replaces the field's selection; `end` puts the nodes at the end of the field (P17) |
| host to page | `pasteScan` | `loadId`, `html`, `mode` (`strip`, `keep` or `internal`) | `sources`: one entry per `img` element of the parsed tree, in document order, after P14.1's removals unless `mode` is `internal`: its `src` attribute's value, or null when it has none. The page parses `html` as P14 says and changes nothing |
| host to page | `pasteFilter` | `loadId`, `html` (the text `pasteScan` got), `mode`, `dark` (boolean: the effective appearance, P15.2), `rewrites` (a list of `{index, src}`, `index` into `pasteScan`'s list, `src` already encoded) | `html`: the page parses `html` again, applies P14.1 unless `mode` is `internal`, sets the `src` attribute of each rewritten `img` to its `src` exactly, runs P15 in `mode` (its whitespace and trim steps included) and returns the serialized result, which the host sends in `insert` unchanged. The content of steps 3 and 4 of P9 comes here too, as `internal` with no rewrites. An `index` out of range is refused as B1 says |
| host to page | `fieldAt` | `loadId`, `x`, `y` (CSS pixels in the page's viewport) | `field`: null when no shown field is at that point, else `{ord, collapsed, mode}` (P17) |
| host to page | `setBusy` | `loadId`, `ord`, `busy` (boolean) | done. While `busy` the field takes no input (keys, paste, drop, commands) and carries `aria-busy="true"`; the host shows P10.2's indicator |
| page to host | `fieldDrag` | `loadId`, `ord`, `html` (storage form, as P9 step 1) | when a drag starts from a selection in a rich field (P17) |
| page to host | `contextTarget` | `loadId`, `ord`, `kind` (`text`, `selection` or `image`); for `image` also `src` (the attribute's value as the page holds it, display-encoded), `loaded` (boolean: the image has loaded and decoded), `naturalWidth` and `naturalHeight` (CSS pixels, 0 when not loaded) | on a context-menu press in a rich field (secondary click, long press), before the host shows P19's menu |

The host decodes `contextTarget`'s `src` by `text/media-references` M7 before it tells remote from local (M5) and resolves the file. The page refuses `insert` for an `ord` that is not the focused rich field and replies `changed: false`, like `format` (`editor/formatting` F13).

**Target rule.** Decided 2026-10-27, for Attach, its photo sources, Record Audio…, paste and drop:

1. When the command starts, the host records the page's `loadId` and the `ord` of the focused rich field (for a drop, P17's field). Presenting a source (the file importer, the photo picker, the camera, the recorder panel, P4's size alert) may take focus from the field and, on iPhone and iPad, dismiss the keyboard; the field then blurs and commits as usual (`editor/field-html` S1) and keeps its selection (F4).
2. When the content is ready (the source has returned, P4's adds and alerts and P10.2's fetches have ended), the host sends `setFocus` for the recorded `ord`, which restores the field's selection, and then `insert`.
3. Nothing is inserted when the page's `loadId` is no longer the recorded one, the note has closed, the field is now in HTML mode or collapsed, or a flush holds the page (`editor/field-html` B3). Files already added stay, as P4's "written at once" says, and Check Media reports them as unused (`media/media-files` M13).
4. The content never goes into another field than the recorded one, even when focus has moved there meanwhile.

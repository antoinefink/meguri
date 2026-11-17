# Paste and drop sources (U197)

Spec-writer notes for `specs/editor/paste-sources.md`. Read at tag 26.09.3
(`refs/anki-26.09.3`); no code was copied. Written 2026-11-10 (session
s-20261110T085429Z-165d83d3) by reading both of Anki's editors side by side:
the host editor, which Add, Edit Current and Browse construct by default, and
the page editor, which they construct only when the collection's
experimental-editor flag is on (Shift held at window open inverts the choice;
Add opened from Browse never reads the flag). `editor/paste` P1 is right that
the host editor is the default (S1). Corrected 2026-11-10 after review: the
first draft had Add and Edit Current always on the page editor.

Meguri's own design, with no Anki source: the S8 decision (undecodable image
data yields nothing), every "Meguri" column (they cite `editor/paste`
decisions, including the 2026-11-10 ones on redirects, compression and
`file://` decoding), the proposed deviation ids of S14, and the proposed
driver op of S15.1.

| Spec section | Anki files read | Read for |
|---|---|---|
| paste-sources S1 | `qt/aqt/main.py` (new-or-legacy dialog choice from the flag and Shift), `qt/aqt/__init__.py` (dialog names to classes), `qt/aqt/addcards.py`, `qt/aqt/addcards_legacy.py`, `qt/aqt/editcurrent.py`, `qt/aqt/editcurrent_legacy.py`, `qt/aqt/browser/browser.py` (editor class choice; Add opened without the flag), `pylib/anki/collection.py` (experiment flag default), `qt/aqt/editor.py`, `qt/aqt/editor_legacy.py`, `ts/routes/editor/rich-text-input/rich-text-resolve.ts` | which window constructs which editor by default and with the flag; the page forwarding paste to the host in the host editor |
| paste-sources S2 | `qt/aqt/editor_legacy.py` (mode with Shift), `ts/routes/editor/rich-text-input/data-transfer.ts` (mode, remembered Shift from key presses: one value for the page), `ts/routes/editor/rich-text-input/rich-text-resolve.ts` (every rich input registers the key handler), `ts/lib/tslib/keys.ts` | the config key read per paste; the two Shift sources |
| paste-sources S3 | `qt/aqt/editor_legacy.py` (mime processing order, drop), `ts/routes/editor/rich-text-input/data-transfer.ts` (data-transfer processing order, URL list reading) | step order of both paths; empty HTML; empty URL lines; the case of the `file://` test |
| paste-sources S4 | `qt/aqt/editor_legacy.py` (URL to link, URL test), `ts/routes/editor/rich-text-input/data-transfer.ts` (URL to link, URL test) | media-URL suffix test on the whole text; link text decoding (full unquote versus the standard URI decode); unescaped `href` |
| paste-sources S5 | `qt/aqt/editor_legacy.py` (URL retrieval), `pylib/anki/httpclient.py` (timeout, User-Agent, session with default compression), `rslib/src/editor.rs` (remote and local download, status check that refuses only 4xx and 5xx), `Cargo.toml` and `Cargo.lock` (HTTP client built without compression support), `rslib/src/backend/media.rs` (add-from-URL call, error text), `rslib/src/error/network.rs` (status and network error mapping, 407 and 501 included), `ftl/core/editing.ftl`, `ftl/qt/qt-misc.ftl` | status rules (200 only versus not 4xx or 5xx), unfollowed 3xx, timeout kinds, redirect limits of the two HTTP clients, compression, User-Agents, what the alerts say (described, not quoted) |
| paste-sources S6, S6.1 | `qt/aqt/editor_legacy.py` (query strip, unquote then basename), `pylib/anki/media.py` (extension from media type; Python's extension split, which skips leading dots), `rslib/src/editor.rs` (URL parse, last segment then decode, extension from media type; Rust's path extension rule), `rslib/src/backend/media.rs` (returned name), `rslib/src/media/files.rs` (add uniquely, lowercase retry, hash suffix) | name derivation per path; exact media-type match; the two "has no extension" tests (`..png`); the asked-for name returned instead of the stored one |
| paste-sources S7 | `qt/aqt/editor_legacy.py` (file name to snippet: image extension or a played sound tag for everything else; save hooks), `ts/routes/editor/rich-text-input/data-transfer.ts` (file name to snippet), `ts/routes/editor/NoteEditor.svelte` (save transform), `pylib/anki/media.py` (IRI escaping), `rslib/src/text.rs` (IRI encode and decode) | the two `src` encodings; the save's decode in both paths; raw `src` after a fetch rewrite |
| paste-sources S8 | `qt/aqt/editor_legacy.py` (pasted image read, JPEG and PNG quality, name), `qt/aqt/mediasrv.py` (pasted image conversion, the same quality settings), `ts/routes/editor/rich-text-input/data-transfer.ts` (image types list, checksum name) | image sources and order; JPEG 80 and PNG; name from the encoded bytes; empty output when the toolkit cannot decode |
| paste-sources S9 | `ts/routes/editor/rich-text-input/data-transfer.ts` (audio file step, playback) | name-based audio test; no audio step in the host path |
| paste-sources S10 | `qt/aqt/editor_legacy.py` (text processing), `ts/routes/editor/rich-text-input/data-transfer.ts` (text processing, inlined image tokens) | token split and its whitespace sets (Python's and JavaScript's), escaping, tabs, space runs, keep-mode tokens |
| paste-sources S11 | `qt/aqt/editor_legacy.py` (clipboard change tracking, internal reuse, media-server rewrite), `ts/routes/editor/rich-text-input/data-transfer.ts` (remembered cut and copy read from the event's own data, pre-filter rewrite), `ts/routes/editor/rich-text-input/rich-text-resolve.ts` (copy and cut handlers with no earlier write to the event data) | when field content is remembered and forgotten in each path; the page column rests on the Clipboard API's copy-event order (the event's data is empty while it is dispatched), to be confirmed by the editor-page oracle |
| paste-sources S12 | `qt/aqt/editor_legacy.py` (drop: snippets made before the field check, the HTML pre-pass only after it; any drag whose source is in the app taken as internal), `ts/routes/editor/NoteEditor.svelte` (focus at point, collapsed and hidden rich text), `ts/routes/editor/rich-text-input/data-transfer.ts` (drop handler) | drop target, collapsed fields, drags from inside the app |
| paste-sources S13 | the files of S5, S6, S8 and S10; `qt/aqt/editor_legacy.py` and `ts/routes/editor/rich-text-input/data-transfer.ts` (inlined image to file name: empty for other types, assigned to `src`); `rslib/src/media/files.rs` (name normalization) | what each failure leaves in the field and the folder |
| paste-sources S14 | none | Meguri's summary of S2 to S13 |
| paste-sources S15, S15.1 | `rslib/src/backend/media.rs`, `proto/anki/media.proto` (add-from-URL request and response) | that the page editor's fetch is one collection-library call, so a driver op can observe it |
| paste-sources S5 (encoded `#` row), S6 (redirect sentence), S14 (two rows) | `qt/aqt/editor_legacy.py` (local file read: unquote, then `%` and `#` escaped again) for the host column; the page column from the goldens of `editor/paste/url/file` and `editor/paste/url/status` (oracle maintainer, 2026-11-10) | added after recording |
| paste-sources S2, S3, S5, S6, S7, S10 to S14 (review corrections, 2026-11-10) | the files listed in those rows | rewording of S6 and S10 as output rules; alert texts described instead of quoted; S3's two-line URL item described neutrally; the rows added for 3xx, compression, `..png`, `play`, PNG quality, 407 and 501, the `data:image/webp` `img`, and the corrected S2, S11 and S12 cells |

`specs/editor/paste.md`, 2026-11-10 review additions (P2 percent-decoding,
P8 commands, P9 item selection and text decoding, P9.1, P10.2 core service,
redirects, compression and busy state, P10.3 naming source, P11, P14 and P25
split, P14.2 both modes, P16 order, P17 drags): Meguri's own design. The Anki
facts they cite (both pre-passes run in both modes; the space-run layout puts
no-break spaces first; the default editor's drop never moves the source) come
from the files of S5, S10 and S12 above.

## Session s-20261117T152613Z-96466c16 (spec writer): deferred harvest rows, batch 2

Read at tag 26.09.3 (`refs/anki-26.09.3`) to classify deferred harvest rows, write their scenarios and draft spec text; no code, test text or fixture value was copied unless a row says otherwise, and every expected value comes from the recorded oracle goldens.

| Spec section and scenarios | Anki file | Read for |
|---|---|---|
| editor/paste P2, P3, P7, P11, P13, P14.2 and editor/paste-sources S7, S8, S9, S10 checked against the page-editor data-transfer obligations (h4d2c40a0930e, h88ee1a8ec135, hbed095c14b47, hc53d1db14186, hcddb2db9708d, hd319020c121d) | `ts/routes/editor/rich-text-input/data-transfer.test.ts`, `ts/routes/editor/rich-text-input/data-transfer.ts` | Which behavior must be proven, and whether the specs agree with the pinned page editor on four points: the audio/video and image extension lists with their case-insensitive last-dot extension rule; the bare name versus `[sound:]` snippet and the playback call; the pasteImagesAsPng extension choice with the `paste-<sha1>.<ext>` name taken from the converted bytes; and decoding of `data:image/{jpg,jpeg,png,gif};base64` for text tokens and img sources. They agree, so no new spec text. |
| triage of the attach and record toolbar rows to editor/paste P5, P20, P24 snippets, P25, and the proposed P25 Meguri-tests line | `ts/tests/e2e/media-buttons.spec.ts` | Triage only: Attach and Record insert the added media into the field that was focused when the command started. Classified native-ui. |

# Text area specs and the `text` observation (U20)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to write `specs/text/*.md` and
`driver/observe/text.py`. No code was copied; every rule in the specs was
re-derived in the spec writer's words and checked against oracle goldens.

| Spec section | Anki file | Read for |
|---|---|---|
| strip-html H2-H4 | `rslib/src/text.rs` | markup and entity stripping order, all-or-nothing decoding, U+00A0 handling |
| strip-html H5 | `rslib/src/text.rs` | media names kept in place of media tags |
| strip-html H6 | `rslib/src/text.rs`, `rslib/src/browser_table.rs`, `rslib/src/import_export/text/csv/export.rs` | the one-line text and where it is used |
| media-references M2-M4 | `rslib/src/text.rs` | media tag and sound tag syntax (verbose-mode pattern: a space in a bracket class is ignored), extraction order, per-name decoding |
| media-references M5 | `rslib/src/media/check.rs`, `rslib/src/import_export/gather.rs` | which consumers skip remote names |
| media-references M6-M7 | `rslib/src/text.rs`, `Cargo.lock` (`percent-encoding-iri` fork) | display encoding set, replacement inside the whole tag, lossy decoding |
| media-references M8 | none | derived from M2, M6 and M7; conditions 3 and 4 came from Meguri's corpus and property tests, and every example is in the `refs/iri` and `refs/extract` goldens |
| media-filenames F1-F4 | `rslib/src/media/files.rs`, `rslib/src/sync/media/mod.rs`, `Cargo.lock` (`unic-ucd-category` 0.9.0) | transform steps, 120-byte limit, Unicode 10 unassigned set, store-only rules |
| field-emptiness E1-E3 | `rslib/src/template.rs`, `rslib/src/notetype/cardgen.rs`, `rslib/src/notes/mod.rs` | spacer-tag pattern, use after save normalization |
| checksum-and-sort-field C1-C5 | `rslib/src/notes/mod.rs`, `rslib/src/storage/note/add.sql`, `rslib/src/storage/schema11.sql`, `rslib/src/config/bool.rs` | save normalization, sort field and checksum, text binding of `sfld`, `normalize_note_text` default, first-field checks |
| guid G1-G3 | `rslib/src/notes/mod.rs`, `pylib/anki/utils.py` | alphabet, digit order, zero, when GUIDs are assigned |
| unicode-primitives U2 | `rslib/src/storage/sqlite.rs`, `Cargo.lock` (`unicase` 2.6.0) | the collation's comparison; the three-character fold order comes from the crate itself |
| description-html | `rslib/src/markdown.rs`, `rslib/src/card_rendering/service.rs`, `qt/aqt/overview.py`, `qt/aqt/deckdescription.py` | context only: Anki renders `md` descriptions with plain CommonMark and shows other descriptions unsanitized; Meguri's design is its own |
| driver `text` observation | `proto/anki/card_rendering.proto`, `proto/anki/media.proto`, `rslib/src/card_rendering/service.rs`, `rslib/src/media/service.rs`, `rslib/src/media/mod.rs`, `pylib/anki/media.py` | the backend calls that expose each function, and that adding a media file also lowercases new names |

Third-party crate sources read for exact behavior (not Anki code):
`htmlescape` 0.3.1 (`decode_html` grammar, HTML 4 entity table without
`apos`), `unicase` 2.6.0 (fold iterator emits three-character folds rotated),
`percent-encoding` 2.3.2 (upstream encodes all non-ASCII bytes, unlike the fork).

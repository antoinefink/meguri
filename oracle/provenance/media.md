# Media files: adding, registration, trash (U31)

Spec-writer notes for `specs/media/media-files.md` and
`specs/conformance/media-ops.md`. Read at tag 26.09.3 (`refs/anki-26.09.3`); no
code was copied. Black-box probes ran in the oracle image
(`meguri-oracle:26.9.3-82836f1eb53e6a60`) through the `anki` Python API with no
network; a dry run of every `conformance/scenarios/media/` scenario there gave
the outputs the deviations in `conformance/deviations.yaml` start from.

| Spec section | Anki files read | Read for |
|---|---|---|
| media-files M1 | `pylib/anki/media.py` (media folder and `.db2` paths), `rslib/src/media/files.rs` (trash folder next to the media folder), `rslib/src/collection/mod.rs` | folder layout and names |
| media-files M2, M3 | `rslib/src/media/files.rs`, `rslib/src/media/mod.rs`, `rslib/src/media/service.rs` | the add decision: the existing-file check on the volume, the lowercase retry that runs the transform again, the hash rename, what the add records |
| media-files M4 | `rslib/src/media/files.rs`, `rslib/src/sync/media/mod.rs` (120-byte limit) | hash-name stem and extension limits, the trailing `.` with no extension, no post-processing |
| media-files M5 | `rslib/src/media/check.rs` (case-insensitive reference matching), `rslib/src/sync/media/database/client/mod.rs` (lowercased checksum lookup) | how Anki itself matches names regardless of case |
| media-files M7 | `rslib/src/sync/media/database/client/changetracker.rs`, `rslib/src/sync/media/database/client/mod.rs`, `rslib/src/sync/media/mod.rs`, `rslib/io/src/lib.rs` (directory iteration skips non-files) | which entries register, the Apple NFC handling, the size limits, the folder-mtime shortcut and when the marker is updated, whole-second file times |
| media-files M7.3 | `rslib/src/sync/media/syncer.rs`, `rslib/src/import_export/package/apkg/import/media.rs`, `rslib/src/import_export/package/colpkg/import.rs` | when Anki registers changes |
| media-files M8 | `rslib/src/media/files.rs` | the ignored-name pattern (verbose, case-insensitive, `.` unescaped, `(:?` typo for a group) |
| media-files M10 | `rslib/src/media/files.rs`, `rslib/src/media/check.rs`, `rslib/src/media/service.rs`, `qt/aqt/mediacheck.py` | trash move (clobbers, stops at a missing name), restore (rename or re-add), empty |
| media-files M11 | `rslib/src/media/mod.rs`, `rslib/src/sync/media/upload.rs` | which writes set the dirty flag; upload drops non-normalized, empty and oversized entries |
| media-files M14 | probes | the case-variant and trash outcomes on the oracle's case-sensitive volume |
| media-ops F11 | `rslib/src/sync/media/syncer.rs`, `pylib/anki/collection.py` | registration is the first step of a media sync |

Black-box results the spec relies on (oracle image, Linux, case-sensitive tmpfs):

- `Photo.JPG` over `photo.jpg` with the same bytes gives `photo.jpg`; other bytes give `photo-<sha1>.jpg`; an empty folder lowercases (`Diagram.PNG` → `diagram.png`).
- `README` then other bytes: `readme-<sha1>.`; `.PNG`: `-<sha1>.png`; `.` twice: `._` then `-<sha1>._`; `x.abcdefghi jkl` twice: `x-<sha1>.abcdefghi ` (trailing space).
- `İ` × 60 gives `i` U+0307 × 39 + `i`: the lowercased name goes through the transform again.
- Final sigma: `AΣ` → `aς`, `ABΣ.png` → `abσ.png`.
- A full registration (meta `dirMod` zeroed, then a media sync to an unreachable endpoint, which registers before the network call) records hash names ending in `.` or space as deletions.
- Registration ignores `Thumbs.DB`, `.DS_Store`, `thumbs_db`, `thumbſ-db`, `xds_store`, `ſDS_STORE`, empty files, subdirectories, non-normalized names (`a:b.png`, `con.jpg`, `trail.`, 121-byte names) and a 104,857,601-byte file; it registers `my.ds_store`, `thumbs.db.png`, `_font.ttf`, `Upper.PNG` and a 104,857,600-byte file.
- A rewrite within the same second and without a folder change is not seen (hence `media-ops.md` F12).
- Trashing the same name twice keeps only the newer bytes; restore of a name now holding other bytes re-adds under the hash name; identical bytes are dropped.

Driver hints for the oracle maintainer (U31 role work):

- `add_media`: `col.media.write_data(name, bytes)` (the add-media-file backend call). `trash_media`, `restore_media_trash`, `empty_media_trash`: `col.media.trash_files`, `restore_trash`, `empty_trash`.
- The media folder is `col.media.dir()`; the trash is `media.trash` in its parent directory and may not exist (`[]`).
- `media_pending`: zero `meta.dirMod` in `collection.media.db2` when the file exists, start `col.sync_media` with a `SyncAuth` whose endpoint cannot be reached (the worker has no network), wait until `media_sync_status()` is inactive or raises, then read `media` rows with `dirty = 1` (`csum` hex, NULL for a deletion). The registration runs before the network call and commits on its own. Never compare against the sync error text.
- `media_files` and `media_outside`: write with plain file I/O, then `os.utime` every written file and the folder to the frozen clock's instant.
- Check under the frozen clock that three recordings agree, in particular for `trash/trash-restore-empty` and `register/outside-changes`.

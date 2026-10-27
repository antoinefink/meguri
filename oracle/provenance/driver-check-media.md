# Oracle driver: Check Media and check_db failed (F14, K10)

Read at tag 26.09.3 to choose the `anki` calls behind `check_media`,
`delete_unused_media` and `tag_missing_media` (`driver/ops/check_media.py`,
registered in `driver/ops/__init__.py`), the media-case-variants rules in
`driver/ops/media.py` (byte comparison after NFC, and a probe file that tells
whether the media folder is case-sensitive), and the `check_db` rejection
`failed` with its reopen (`driver/ops/maintenance.py`). Meanings are
`specs/conformance/media-ops.md` F14 and
`specs/conformance/collection-check.md` K10. No code was copied.

| Anki file | Read for |
|---|---|
| `pylib/anki/media.py` (`check`, `trash_files`, `dir`), `proto/anki/media.proto` (`CheckMediaResponse`), `rslib/src/media/service.rs` (`check_media`) | the response gives unused, missing and the notes with missing media as values; everything else is only in the report text |
| `rslib/src/media/check.rs` (`summarize_output`, the folder check, `normalize_file`), `rslib/i18n/src/lib.rs` (`without_unicode_isolation`), `ftl/core/media-check.ftl` | the report's lines and their order; summary lines keep the isolation marks, file lines drop them; of two case variants Anki keeps the one it meets first (lower case first) and renames the other to its lower-case name, with a hash suffix when the bytes differ |
| `rslib/src/media/files.rs` (`disallowed_char`, `normalize_nfc_filename`) | `>` is removed from a name, so an old name may hold the renamed line's separator and a new name never does |
| `rslib/src/notetype/checks.rs` (`report_media_field_referencing_templates`, `Template::as_str`), `rslib/src/notetype/mod.rs` (`get_all_notetypes`), `ftl/core/card-templates.ftl` | the template list's header and lines, the front and back names, and that Anki lists templates in its own order (the driver sorts them as C9 says) |
| `qt/aqt/mediacheck.py` (`_on_trash_files`, `add_missing_media_tag`), `qt/aqt/operations/tag.py`, `pylib/anki/tags.py` (`bulk_add`) | the desktop trashes unused files 25 names per call and tags missing-media notes with the collection's bulk tag add, tag `missing-media` |
| `pylib/anki/collection.py` (`fix_integrity`), `rslib/src/collection/service.rs`, `rslib/src/dbcheck.rs` (`check_database`) | Check Database runs its repairs in one transaction after the quick check and optimize; an error other than the corrupt DB error leaves that call |
| `rslib/src/collection/transact.rs`, `rslib/src/undo/mod.rs`, `rslib/src/collection/mod.rs` | the error rolls the repairs back and empties the undo history and study queues, but nothing clears the deck and note type caches, so the driver reopens |

Probed in the oracle image (`driver/tests/test_check_media.py` and the
goldens of `conformance/goldens/maintenance/check-media/` and `check-db/`):
the report's lists match the response's; a name holding `>` is renamed
without it and the rename shows in the folder and the notes; a second run
reports no rename; a note already tagged is not counted again. `Cat.jpg` and
`cat.jpg` are refused as variants without the tag and kept apart with it on
the image's case-sensitive folder, where Anki merges same bytes into
`cat.jpg` and gives other bytes a hash-suffixed name. A review-log `ivl`
outside i32, a taken id and a refused deck add stop Check Database with
`failed`, and the rows are as they were.
